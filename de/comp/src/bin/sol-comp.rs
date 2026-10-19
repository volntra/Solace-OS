//! sol-comp: the Solace compositor.
//!
//!   sol-comp [--nested] [-- COMMAND [ARGS...]]
//!
//! `--nested` (the only backend so far) runs inside another desktop. A
//! command after `--` is started as a client of the new compositor.

use std::process::{Command, ExitCode};

use smithay::reexports::{calloop::EventLoop, wayland_server::Display};
use solcomp::{Comp, LoopData};

fn main() -> ExitCode {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let mut args = std::env::args().skip(1);
    let mut client: Vec<String> = Vec::new();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--nested" => {}
            "--" => client = args.by_ref().collect(),
            "-h" | "--help" => {
                println!("usage: sol-comp [--nested] [-- COMMAND [ARGS...]]");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("sol-comp: unknown argument {other}");
                return ExitCode::from(2);
            }
        }
    }
    match run(&client) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("sol-comp: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(client: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut event_loop: EventLoop<LoopData> = EventLoop::try_new()?;
    let display: Display<Comp> = Display::new()?;
    let display_handle = display.handle();
    let state = Comp::new(&mut event_loop, display)?;
    let mut data = LoopData {
        state,
        display_handle,
    };
    solcomp::backend::winit::init(&mut event_loop, &mut data)?;
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
    event_loop.run(None, &mut data, |_| {})?;
    Ok(())
}
