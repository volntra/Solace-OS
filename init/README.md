# init

`fern-init` (PID 1 and service manager), `fernctl`, and fern-log. Crate
`fern-init` (library `ferninit`). Unit format: `docs/formats/unit.md`.

- `src/unit.rs`, `graph.rs`, `policy.rs`, `manager.rs`, `proto.rs`,
  `log.rs`: the decisions, pure and unit-tested.
- `src/runtime.rs`: the event loop that carries them out.
- `src/pid1.rs`: what only PID 1 does (mounts, root assembly from btrfs
  subvolumes, first-boot /etc, reboot).
- `tests/supervise.rs`: runs the real binaries against a scratch root.

Try it without a VM: `cargo run -p fern-init --bin fern-init -- --root /tmp/r`
with units in `/tmp/r/usr/lib/fern/units/`, then
`cargo run -p fern-init --bin fernctl -- --root /tmp/r status`.
