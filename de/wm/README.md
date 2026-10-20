# de/wm

fern-wm: the window manager (CLAUDE.md 6.6), a separate process that
fern-comp starts and restarts. It speaks the private `fern_wm_v1` protocol
(`docs/protocols/wm.md`).

- `src/policy.rs`: every decision, as events in and commands out, unit
  tested without a compositor (`cargo test -p fern-wm`)
- `src/lib.rs`: carries protocol events to the policy and its commands back
- `src/bin/fern-wm.rs`: the process (connects on `WAYLAND_SOCKET`)

See `docs/STATUS.md` for what it does so far.
