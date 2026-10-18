# Protocols

Prose specs for our private interfaces. Wire definitions (Wayland XML,
D-Bus introspection XML) live next to the code; these documents are the
source of truth for behavior.

| Spec | Interface | Written in |
|---|---|---|
| `wm.md` | core <-> WM private Wayland protocol | Phase 4 |
| `pkgd.md` | `org.sol.Pkg1` D-Bus (polkit-guarded) | Phase 1 (CLI), Phase 3 (daemon) |
| `settingsd.md` | `org.sol.Settings1` D-Bus get/set/watch | Phase 6 |
| `init.md` | solctl <-> sol-init control socket, notify fd | Phase 2 |

None are written yet.
