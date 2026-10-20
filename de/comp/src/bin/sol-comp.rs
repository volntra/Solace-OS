//! sol-comp: the Solace compositor.
//!
//!   sol-comp [--nested | --drm] [-- COMMAND [ARGS...]]
//!
//! `--drm` drives the display (KMS, libinput, libseat); `--nested` runs
//! inside another desktop. Without either, sol-comp nests when it finds a
//! desktop to nest in (`WAYLAND_DISPLAY` or `DISPLAY`) and takes the
//! display otherwise. A command after `--` is started as a client.

use std::io::IsTerminal;
use std::process::{Command, ExitCode};

use smithay::reexports::{calloop::EventLoop, wayland_server::Display};
use solcomp::{Comp, LoopData};

const USAGE: &str = "usage: sol-comp [--nested | --drm] [-- COMMAND [ARGS...]]";

#[derive(Clone, Copy, Debug, PartialEq)]
enum Backend {
    Nested,
    Drm,
}

fn main() -> ExitCode {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    // Logs go to stderr (stdout is for output a caller asked for), in
    // colour only on a terminal: `just comp-drm` keeps them in a file.
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .init();

    let mut args = std::env::args().skip(1);
    let mut client: Vec<String> = Vec::new();
    let mut backend = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--nested" => backend = Some(Backend::Nested),
            "--drm" => backend = Some(Backend::Drm),
            "--" => client = args.by_ref().collect(),
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("sol-comp: unknown argument {other}\n{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let backend = backend.unwrap_or_else(|| {
        let nested = ["WAYLAND_DISPLAY", "DISPLAY"]
            .iter()
            .any(|v| std::env::var_os(v).is_some_and(|s| !s.is_empty()));
        if nested {
            Backend::Nested
        } else {
            Backend::Drm
        }
    });
    match run(backend, &client) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("sol-comp: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(backend: Backend, client: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut event_loop: EventLoop<'static, LoopData> = EventLoop::try_new()?;
    let display: Display<Comp> = Display::new()?;
    let display_handle = display.handle();
    let state = Comp::new(&mut event_loop, display)?;
    let mut data = LoopData {
        state,
        display_handle,
    };
    match backend {
        #[cfg(feature = "nested")]
        Backend::Nested => solcomp::backend::winit::init(&mut event_loop, &mut data)?,
        #[cfg(feature = "drm")]
        Backend::Drm => solcomp::backend::drm::init(&mut event_loop, &mut data)?,
        #[allow(unreachable_patterns)]
        other => return Err(format!("built without the {other:?} backend").into()),
    }
    tracing::info!(socket = ?data.state.socket_name, "listening");
    if let Some((cmd, args)) = client.split_first()
        && let Err(e) = Command::new(cmd)
            .args(args)
            .env("WAYLAND_DISPLAY", &data.state.socket_name)
            .spawn()
    {
        // The compositor keeps running; the client is the one that failed.
        tracing::warn!("could not start {cmd}: {e}");
    }
    // Replies to client requests (configures, pings, buffer releases) go
    // out after every wake-up, not only when a frame is drawn: an idle
    // display draws nothing, and a new window waits for its configure.
    event_loop.run(None, &mut data, |data| {
        let _ = data.display_handle.flush_clients();
    })?;
    Ok(())
}
