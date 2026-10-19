# Bootstrap plan format (`bootstrap/plan.toml`)

The plan lists build stages in order; `spk bootstrap [plan]` runs them.
Each recipe is built by the normal `spk build` machinery (sandbox,
verification, signing), so every file still comes from a `.solpkg`.

```toml
target = "x86_64-solace-linux-gnu"   # $SOL_TGT; default shown

[[stage]]
name = "cross"                        # unique; also the work subdir
description = "one line"
recipes_dir = "bootstrap/recipes"     # where the stage's recipes live
repo = "build/bootstrap/repo"         # packages go here
dep_repos = []                        # further repos build deps may use
toolchain = "cross"                   # default mode: host | cross | native
base = ["cross-gcc"]                  # installed into every sysroot
recipes = ["cross-binutils", "..."]   # built in exactly this order
```

Build deps are resolved across `repo` and `dep_repos` with the newest
version winning. Temporary packages are versioned `X~bootstrap`, which
sorts below `X`, so a final package replaces its temporary namesake in
every later sysroot as soon as it exists.

## Toolchain modes

| mode | compilers | sandbox sees | used for |
|---|---|---|---|
| `host` | the Nix devShell's | `/nix/store`, sysroot at `/usr` (+ `-I/-L/usr` flags) | Phase 1 scaffolding |
| `cross` | host tools + cross toolchain in `/usr/cross/bin` | `/nix/store`, sysroot at `/usr` | stages 0-1 |
| `native` | the sysroot's own | only the sysroot (`/usr`, `/bin -> usr/bin`, `/lib64 -> usr/lib`, a minimal `/etc`) | everything that ships |
