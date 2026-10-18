# Status

_Last updated: 2026-10-18 (session 1)_

## Current phase: 0 (Scaffold): complete

**Demo:**

```sh
nix develop
just check        # or: just demo
```

`just check` = `cargo fmt --check` + `cargo clippy -D warnings` +
`cargo test` + `solcheck` (repo invariants). Verified green inside
`nix develop` in the cloud dev container on 2026-10-18 (nixpkgs pinned
to `nixos-26.11pre1093333.fe13242281a6`: rustc 1.98.1, just 1.58.0,
QEMU 11.1.2, GRUB 2.14, OVMF 202608). `just nix-check`
(`nix flake check`) also passes.

## Done

- Names fixed: Solace / `sol` / `spk` / `ace` / Koi (CLAUDE.md section 0,
  DECISIONS.md).
- CLAUDE.md with names filled in; `tools/` added to the layout.
- Repo layout from CLAUDE.md section 5: every directory exists with a
  README saying what goes there and which phase fills it. **These are
  empty placeholders, not implementations.**
- `flake.nix` devShell (Rust, just, CMake/Ninja, ccache, bubblewrap, zstd,
  QEMU, OVMF, GRUB EFI, dosfstools/mtools, btrfs-progs, cryptsetup) +
  `flake.lock`.
- `justfile`: `check`, `fmt-check`, `clippy`, `test`, `lint`, `fmt`,
  `demo`, `nix-check`, `doctor`.
- Cargo workspace (edition 2024, clippy pedantic, `unsafe_code = deny`).
- `tools/solcheck`: placeholder, Apple-name/asset, and recipe-justification
  checks, 11 unit tests.
- Docs: DECISIONS.md, this file, HIG.md (outline only), protocols/ and
  formats/ indexes (specs not written yet).

## In progress

Nothing.

## Broken / not verified

- The devShell was verified in the cloud container, not yet on the human's
  NixOS host.

## Open questions for the human

1. **Desktop name:** recorded as DE = Koi, release label "SolOS 1: Koi",
   OS = Solace. Is "SolOS" meant to replace "Solace" as the OS name?
2. **License** for our code (no license chosen yet; Cargo manifests have
   none).
3. **Git remote:** the repo is local to the session container. It needs a
   GitHub (or other) remote to push to, or the work is lost when the
   container is reclaimed.

## Next: Phase 1 (spk core)

Plan, in order:

1. `docs/formats/recipe.md`, `package.md`, `repo.md`.
2. `pkg/` crates: `spk-core` (recipe + manifest types, versions),
   `spk-build` (fetch + sha256 verify, then an unprivileged bubblewrap
   build with no network), `spk-sign` (ed25519 via `ed25519-dalek`),
   `spk-db` (installed-file DB, SQLite), `spk-solve` (`resolvo`), `spk`
   CLI.
3. `.solpkg` = tar.zst + manifest + detached signature; refuse unsigned
   or mismatched packages, with no override flag.
4. `install --root` into a scratch root; `files`, `owns`, `info`,
   `remove`.
5. Failure-injection tests for corrupt archives and bad signatures.
6. 10 small recipes (e.g. zlib, zstd, xz, bzip2, lz4, ncurses, readline,
   bash, coreutils, util-linux) with justifications.
7. Demo recipe `just demo` -> builds and installs the 10 into
   `./build/scratch-root`.
