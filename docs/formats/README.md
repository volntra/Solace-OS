# File formats

| Spec | What | Written in |
|---|---|---|
| `recipe.md` | `recipes/<name>/recipe.toml` | Phase 1 |
| `package.md` | `.fernpkg`: tar.zst + manifest + detached ed25519 signature | Phase 1 |
| `repo.md` | signed repo index | Phase 1 |
| `unit.md` | fern-init service units (TOML) | Phase 2 |
| `bundle.md` | `<Name>.fernapp/` app bundles | Phase 11 |
| `icon.md` | `<app-id>.fernicon/` adaptive icons | Phase 7 |
| `install-profile.md` | installer profile TOML | Phase 12 |

recipe.md, package.md and repo.md are written (Phase 1); the rest are not yet.
