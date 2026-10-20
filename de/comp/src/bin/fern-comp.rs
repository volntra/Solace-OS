//! fern-comp: the Fern compositor.
//!
//!   fern-comp [--nested | --drm] [--wm PATH | --no-wm] [--shell PATH]
//!             [-- COMMAND [ARGS...]]
//!
//! `--drm` drives the display (KMS, libinput, libseat); `--nested` runs
//! inside another desktop. Without either, fern-comp nests when it finds a
//! desktop to nest in (`WAYLAND_DISPLAY` or `DISPLAY`) and takes the
//! display otherwise. A command after `--` is started as a client.
//!
//! The window manager (fern-wm) is started with it and restarted if it
//! exits: `--wm PATH` (or `FERN_WM`) names it, otherwise the `fern-wm` next
//! to this binary, else the one on `PATH`. `--no-wm` runs without one (the
//! core's fallback placement and click-to-focus).
//!
//! `--shell PATH` (or `FERN_SHELL`) starts the shell (menu bar, dock,
//! panels: fern-shell) the same way, trusted with the shell's protocols,
//! and restarts it if it exits. Without it there is no shell.

use std::io::IsTerminal;
use std::os::{fd::OwnedFd, unix::net::UnixStream};
use std::path::PathBuf;
use std::process::{Child, Command, ExitCode, Stdio};

use ferncomp::{Comp, LoopData};
use smithay::reexports::{calloop::EventLoop, wayland_server::Display};

const USAGE: &str = "usage: fern-comp [--nested | --drm] [--wm PATH | --no-wm] [--shell PATH] [-- COMMAND [ARGS...]]";

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
    let mut wm = Some(std::env::var_os("FERN_WM").map_or_else(default_wm, PathBuf::from));
    let mut shell = std::env::var_os("FERN_SHELL")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--nested" => backend = Some(Backend::Nested),
            "--drm" => backend = Some(Backend::Drm),
            "--no-wm" => wm = None,
            "--wm" => {
                let Some(p) = args.next() else {
                    eprintln!("fern-comp: --wm needs a path\n{USAGE}");
                    return ExitCode::from(2);
                };
                wm = Some(PathBuf::from(p));
            }
            "--shell" => {
                let Some(p) = args.next() else {
                    eprintln!("fern-comp: --shell needs a path\n{USAGE}");
                    return ExitCode::from(2);
                };
                shell = Some(PathBuf::from(p));
            }
            "--" => client = args.by_ref().collect(),
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("fern-comp: unknown argument {other}\n{USAGE}");
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
    match run(backend, wm, shell, &client) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("fern-comp: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The `fern-wm` installed next to this binary, else whatever `PATH` finds.
fn default_wm() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("fern-wm")))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("fern-wm"))
}

/// Starts the window manager with one end of a socket pair as its stdin
/// (`WAYLAND_SOCKET=0`) and returns the other end.
fn wm_launcher(path: PathBuf) -> ferncomp::wm::WmLauncher {
    let mut children: Vec<Child> = Vec::new();
    Box::new(move || {
        // Reap the ones that exited (a restart follows an exit).
        children.retain_mut(|c| matches!(c.try_wait(), Ok(None)));
        let (ours, theirs) = UnixStream::pair()?;
        let child = Command::new(&path)
            .stdin(Stdio::from(OwnedFd::from(theirs)))
            .env("WAYLAND_SOCKET", "0")
            .env_remove("WAYLAND_DISPLAY")
            .spawn()?;
        tracing::info!(pid = child.id(), path = %path.display(), "window manager started");
        children.push(child);
        Ok(ours)
    })
}

/// Starts the shell with one end of a socket pair as its stdin
/// (`WAYLAND_SOCKET=0`); `WAYLAND_DISPLAY` stays set for the apps it
/// launches.
fn shell_launcher(path: PathBuf, socket: std::ffi::OsString) -> ferncomp::layers::ShellLauncher {
    let mut children: Vec<Child> = Vec::new();
    Box::new(move || {
        children.retain_mut(|c| matches!(c.try_wait(), Ok(None)));
        let (ours, theirs) = UnixStream::pair()?;
        let child = Command::new(&path)
            .stdin(Stdio::from(OwnedFd::from(theirs)))
            .env("WAYLAND_SOCKET", "0")
            .env("WAYLAND_DISPLAY", &socket)
            .env("QT_QPA_PLATFORM", "wayland")
            // Its warnings go to our log (fern-log), never a journal.
            .env("QT_FORCE_STDERR_LOGGING", "1")
            .spawn()?;
        tracing::info!(pid = child.id(), path = %path.display(), "shell started");
        children.push(child);
        Ok(ours)
    })
}

fn run(
    backend: Backend,
    wm: Option<PathBuf>,
    shell: Option<PathBuf>,
    client: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
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
        Backend::Nested => ferncomp::backend::winit::init(&mut event_loop, &mut data)?,
        #[cfg(feature = "drm")]
        Backend::Drm => ferncomp::backend::drm::init(&mut event_loop, &mut data)?,
        #[allow(unreachable_patterns)]
        other => return Err(format!("built without the {other:?} backend").into()),
    }
    tracing::info!(socket = ?data.state.socket_name, "listening");
    if let Some(path) = wm {
        data.state.set_wm_launcher(wm_launcher(path));
    }
    if let Some(path) = shell {
        let socket = data.state.socket_name.clone();
        data.state
            .set_shell_launcher(shell_launcher(path, socket))?;
    }
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
