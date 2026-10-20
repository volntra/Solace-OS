//! fern-wm: started by fern-comp, which hands it a connection on the
//! file descriptor named by `WAYLAND_SOCKET` (docs/protocols/wm.md).

use std::process::ExitCode;

fn main() -> ExitCode {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
    let conn = match wayland_client::Connection::connect_to_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("fern-wm: no connection to fern-comp ({e}); it is started by fern-comp");
            return ExitCode::FAILURE;
        }
    };
    match fernwm::run(&conn) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // The core going away ends the window manager normally.
            tracing::info!("fern-wm: {e}");
            ExitCode::SUCCESS
        }
    }
}
