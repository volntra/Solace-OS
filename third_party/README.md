# third_party

Upstream code we carry a fix for until a release has it. Each directory is
an unmodified upstream release except for the patches listed here; drop it
(and its `[patch.crates-io]` line in Cargo.toml) once a release includes
them.

- `smithay/`: Smithay 0.7.0 (MIT, `smithay/LICENSE.txt`), examples and
  benches left out. One fix, from upstream master (unreleased): the
  wlr-layer-shell pre-commit check skips a layer surface that was
  destroyed (`src/wayland/shell/wlr_layer/handlers.rs`, marked "Fern
  patch"). Without it, a client that destroys a layer surface and then
  commits the plain `wl_surface` (Qt does, when a shell panel closes) is
  disconnected with "width 0 requested without setting left and right
  anchors".
