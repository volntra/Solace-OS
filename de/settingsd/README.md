# de/settingsd

fern-settingsd: typed settings over D-Bus with TOML storage (CLAUDE.md
6.10), and `fern-settings`, its command-line client. Interface, types and
files: `docs/protocols/settings.md`. The schema is `src/schema.rs`.

Tests: `cargo test -p fern-settingsd` (the bus tests start a private
dbus-daemon).
