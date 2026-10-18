# Decisions

Every non-trivial decision: date, decision, alternatives, why. Newest at the
bottom of the log. The stack in CLAUDE.md section 3 is locked; changes to it
are logged here with the reason.

## Package justifications

One line per recipe in `recipes/`, in the form ``- `name`: why it is in the
base``. `just lint` fails if a recipe has no entry, an entry is empty, or an
entry names a recipe that does not exist.

(none yet: recipes start in Phase 1)

## Log

### 2026-10-18: Names

- **Decision:** OS = **Solace**, prefix = `sol` (D-Bus `org.sol.*`, config
  paths `/etc/sol`, `~/.config/sol`), package manager = `spk`, shell = `ace`,
  desktop umbrella = **Koi**.
- **Interpretation to confirm:** for the desktop name the human answered
  "SolOS 1: Koi". Read as: the desktop is called **Koi**, and
  "SolOS 1: Koi" is the release label for version 1 (OS name stays Solace,
  as chosen in the first answer). If "SolOS" was meant to replace "Solace"
  as the OS name, that is a search-and-replace in CLAUDE.md and the docs
  today; it gets more expensive once branding art exists.
- **Alternatives offered:** Lumen, Halcyon, Orbit; shell names calm, hush,
  solsh, ember; desktop names Harbor, Meadow, Glow, Sanctum.

### 2026-10-18: One Cargo workspace at the repo root

- **Decision:** all Rust crates (spk, sol-init, sol-bootgen, sol-comp, ...)
  are members of one workspace in `/Cargo.toml`, sharing `Cargo.lock`, lint
  settings, and the `target/` dir.
- **Alternatives:** one workspace per top-level dir; independent crates.
- **Why:** one lockfile means one audited dependency set; `just check` is a
  single clippy/test invocation; shared crates (logging, D-Bus types,
  settings schema) are path deps without publishing.

### 2026-10-18: Lints and edition

- **Decision:** Rust edition 2024, `rust-version = "1.88"` (let-chains),
  `clippy::pedantic` warn workspace-wide (two noisy lints allowed), and
  `unsafe_code = "deny"` by default. Crates that need FFI (compositor, icon
  C API, PID 1 syscalls) opt in per module with `#[allow(unsafe_code)]` and
  a `// SAFETY:` comment on every block.
- **Why:** `just check` runs clippy with `-D warnings`, so the bar is
  enforced, not advisory. Default-deny makes every unsafe block a visible,
  reviewed choice, which matters for PID 1 and the privileged daemons.

### 2026-10-18: `tools/` directory and `solcheck`

- **Decision:** added `tools/` (not in the brief's layout) holding
  `solcheck`, a std-only Rust binary run by `just lint`. It checks: no
  unfilled name placeholders anywhere; no Apple trademarked names in shipped
  paths (`apps/ assets/ boot/ bootstrap/ de/ image/ init/ pkg/ recipes/
  sh/`) unless the line says `solcheck: allow-name`; no Apple font or icon
  files (`SF*`/`NewYork*` fonts, `.icns`, `.car`) anywhere; every recipe has a
  justification above; required docs exist.
- **Alternatives:** grep in the justfile; a pre-commit hook.
- **Why:** rules 9 and 6.3 are easy to break silently. A tested tool with
  clear messages is more reliable than regexes in a justfile, and it costs
  no dependencies.

### 2026-10-18: Nix devShell and nixpkgs pin

- **Decision:** `flake.nix` uses nixpkgs **nixos-unstable** from the
  channel tarball (`https://channels.nixos.org/nixos-unstable/nixexprs.tar.xz`),
  locked in `flake.lock` to a specific release
  (`nixos-26.11pre1093333.fe13242281a6`, narHash pinned). Rust comes from
  nixpkgs (no rust-overlay/fenix).
- **Alternatives:** `github:NixOS/nixpkgs/...` (the usual form); a stable
  release branch; rust-overlay for a pinned toolchain.
- **Why:** unstable has the newest Smithay-compatible Mesa/libdrm and Qt 6,
  which Phases 4-6 need. The tarball form locks without the GitHub API (the
  cloud dev container cannot reach it) and pins just as strictly. nixpkgs
  Rust is recent enough for edition 2024. One less input to maintain.
- **Scope:** the devShell currently holds only Phase 0-3 tooling (Rust,
  just, CMake/Ninja, ccache, bubblewrap, zstd, QEMU, OVMF, GRUB EFI, ESP and
  btrfs tools). Qt 6, Smithay's system deps (libinput, libseat, Mesa,
  wayland), and greetd are added in the phase that first needs them, to keep
  the shell fast to enter until then.

### 2026-10-18: Flake also builds repo tooling hermetically

- **Decision:** `packages.solcheck` and `checks.solcheck` build and test
  solcheck with `rustPlatform.buildRustPackage`; `just nix-check` runs
  `nix flake check`.
- **Why:** catches "works in my target/ dir" problems, and sets the
  pattern for later crates.
