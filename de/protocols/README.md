# de/protocols

Private Wayland protocol XML and its generated bindings (the
`fern-wm-proto` crate: `server` for fern-comp, `client` for fern-wm).

- `fern-wm-v1.xml`: the core <-> window manager protocol; prose spec in
  `docs/protocols/wm.md`
- `fern-shell-v1.xml`: the core <-> shell protocol (rounded blur behind
  shell surfaces, wallpaper, global shortcuts, the active window's
  menu address); prose spec in `docs/protocols/shell.md`. The shell (C++)
  generates its own bindings with wayland-scanner.
