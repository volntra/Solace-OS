//! Generated bindings for our private protocols: `fern-wm-v1.xml`, between
//! fern-comp and fern-wm (docs/protocols/wm.md), at the top level, and
//! `fern-shell-v1.xml`, between fern-comp and the shell
//! (docs/protocols/shell.md), in [`shell`]. The compositor uses the
//! `server` sides, the others the `client` sides.

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

pub mod shell {
    #[cfg(feature = "server")]
    pub mod server {
        use wayland_server;
        use wayland_server::protocol::*;
        pub mod __interfaces {
            use wayland_server::protocol::__interfaces::*;
            wayland_scanner::generate_interfaces!("fern-shell-v1.xml");
        }
        use self::__interfaces::*;
        wayland_scanner::generate_server_code!("fern-shell-v1.xml");
    }

    #[cfg(feature = "client")]
    pub mod client {
        use wayland_client;
        use wayland_client::protocol::*;
        pub mod __interfaces {
            use wayland_client::protocol::__interfaces::*;
            wayland_scanner::generate_interfaces!("fern-shell-v1.xml");
        }
        use self::__interfaces::*;
        wayland_scanner::generate_client_code!("fern-shell-v1.xml");
    }
}
