# init

`sol-init` (PID 1 and service manager), `solctl`, and sol-log. Crate
`sol-init` (library `solinit`). Unit format: `docs/formats/unit.md`.

- `src/unit.rs`, `graph.rs`, `policy.rs`, `manager.rs`, `proto.rs`,
  `log.rs`: the decisions, pure and unit-tested.
- `src/runtime.rs`: the event loop that carries them out.
- `src/pid1.rs`: what only PID 1 does (mounts, root assembly from btrfs
  subvolumes, first-boot /etc, reboot).
- `tests/supervise.rs`: runs the real binaries against a scratch root.

Try it without a VM: `cargo run -p sol-init --bin sol-init -- --root /tmp/r`
with units in `/tmp/r/usr/lib/sol/units/`, then
`cargo run -p sol-init --bin solctl -- --root /tmp/r status`.
