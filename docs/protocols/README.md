# Protocols

Prose specs for our private interfaces. Wire definitions (Wayland XML,
D-Bus introspection XML) live next to the code; these documents are the
source of truth for behavior.

| Spec | Interface | Written in |
|---|---|---|
| `wm.md` | core <-> WM private Wayland protocol (`fern_wm_v1`) | Phase 5 (written) |
| `pkgd.md` | `org.fern.Pkg1` D-Bus (polkit-guarded) | Phase 3 (daemon) |
| `settingsd.md` | `org.fern.Settings1` D-Bus get/set/watch | Phase 6 |
| `init.md` | fernctl <-> fern-init control socket, notify fd | Phase 2 |

Only `wm.md` is written so far.
