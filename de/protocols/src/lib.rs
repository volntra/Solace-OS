//! Generated bindings for `fern-wm-v1.xml`, the private protocol between
//! fern-comp and fern-wm (docs/protocols/wm.md). The compositor uses the
//! `server` side, the window manager the `client` side.

// Generated code: the scanner's output, not ours to lint.
#![allow(
    unsafe_code,
    missing_debug_implementations,
    clippy::all,
    clippy::pedantic
)]

#[cfg(feature = "server")]
pub mod server {
    use wayland_server;
    pub mod __interfaces {
        use wayland_backend;
        wayland_scanner::generate_interfaces!("fern-wm-v1.xml");
    }
    use self::__interfaces::*;
    wayland_scanner::generate_server_code!("fern-wm-v1.xml");
}

#[cfg(feature = "client")]
pub mod client {
    use wayland_client;
    pub mod __interfaces {
        use wayland_backend;
        wayland_scanner::generate_interfaces!("fern-wm-v1.xml");
    }
    use self::__interfaces::*;
    wayland_scanner::generate_client_code!("fern-wm-v1.xml");
}
