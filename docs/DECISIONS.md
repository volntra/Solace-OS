# Decisions

Every non-trivial decision: date, decision, alternatives, why. Newest at the
bottom of the log. The stack in CLAUDE.md section 3 is locked; changes to it
are logged here with the reason.

## Package justifications

One line per recipe in `recipes/`, in the form ``- `name`: why it is in the
base``. `just lint` fails if a recipe has no entry, an entry is empty, or an
entry names a recipe that does not exist.

- `bash`: /bin/sh and the login shell until ace exists (Phase 10).
- `bc`: the kernel build needs it (timeconst); Gavin Howard's bc, as LFS uses.
- `binutils`: assembler and linker; the system builds itself.
- `coreutils`: the basic userland (ls, cp, ...).
- `diffutils`: diff/cmp, needed by most builds.
- `elfutils`: libelf, required by the kernel's objtool.
- `file`: libmagic, used by builds and later by Files for type detection.
- `findutils`: find/xargs, needed by builds and scripts.
- `flex`: lexer generator, needed to build the kernel and others.
- `gawk`: awk, needed by builds (glibc, kernel).
- `gcc`: the compiler (C, C++) plus gcc-libs (libgcc_s, libstdc++) at runtime.
- `glibc`: the C library (locked stack: NVIDIA userspace and Steam need it).
- `gmp`: GCC's arithmetic library.
- `grep`: needed by builds and scripts.
- `gzip`: .gz archives (kernel image, sources).
- `less`: pager for text output (logs, man pages).
- `linux`: the kernel, with our configuration (boot/kernel/solace.config).
- `linux-headers`: kernel API headers for glibc and everything else.
- `m4`: macro processor needed by autoconf-generated builds and bison.
- `make`: build driver.
- `mpc`: GCC's complex-arithmetic library.
- `mpfr`: GCC's floating-point library.
- `ncurses`: terminal handling for bash, readline, util-linux, less.
- `patch`: applying recipe patches.
- `readline`: line editing for bash and gawk.
- `sed`: needed by builds and scripts.
- `shadow`: user/group/password tools and the text-console login.
- `tar`: unpacking sources; archives.
- `util-linux`: mount, agetty, fdisk, lsblk, and libmount/libblkid.
- `sol-init`: ours (init/): PID 1, service manager, solctl, sol-log.
- `sol-bootgen`: ours (boot/bootgen/): grub.cfg, ESP kernels, initramfs.
- `solace-base`: ours (image/base/): os-release, factory /etc, console units.
- `solace-grub-theme`: ours (boot/grub-theme/): boot menu theme + Inter fonts for GRUB.
- `bzip2`: bzip2 tool and libbz2; libarchive and pcre2grep read .bz2, and many upstream sources ship as .tar.bz2.
- `expat`: XML parser required by dbus-daemon, fontconfig, Mesa, and wayland-scanner.
- `libarchive`: archive library Flatpak requires (it unpacks OCI/bundle layers); provides bsdtar.
- `libffi`: required by wayland-client/server and GLib (needed for the Wayland stack and Flatpak).
- `libxcrypt`: crypt(3) with yescrypt for Linux-PAM and shadow password hashing.
- `lz4`: fast compression for zram swap tooling, libarchive, and kernel/initramfs images.
- `pcre2`: regular expressions for grep, GLib, and Qt.
- `xz`: liblzma and the xz tool; kernel module/initramfs compression, libarchive, and .tar.xz sources.
- `zlib`: deflate; required by almost everything (libpng, Qt, kernel tools, libarchive).
- `zstd`: our package format (.solpkg is tar.zst), btrfs compression tooling, and initramfs compression.

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

### 2026-10-19: Repository and license

- **Decision:** the project lives at https://github.com/volntra/Solace-OS,
  `main` branch. Our code is licensed **GPL-3.0-or-later** (the human's
  repository ships the GPL v3 text; "or later" was the recommended default
  since the human did not pick between -only and -or-later).
- **Note:** switching to GPL-3.0-only is a one-line change in
  `Cargo.toml` while we are the only copyright holders.
- Upstream packages built by `recipes/` keep their own licenses (recorded
  per recipe and per package manifest).

### 2026-10-18: spk database location

- **Decision:** the installed-package database is
  `<root>/usr/lib/spk/db.sqlite` (SQLite, `synchronous=FULL`).
- **Alternatives:** `/var/lib/spk` (the conventional place).
- **Why:** `/usr` is the `@system` subvolume. Keeping the database inside it
  means every system snapshot carries the exact record of its own files,
  and rolling back the system rolls back the database with it. `/var` is a
  separate subvolume that does not roll back.

### 2026-10-18: spk staging lives inside the root's /usr

- **Decision:** downloads, extracted payloads, and backups for a
  transaction live in `<root>/usr/lib/spk/staging/`.
- **Why:** they must be on the same filesystem as their destination so
  placing a file is a `rename(2)` (atomic per file) and backing one up is a
  hard link. `/var` is a different subvolume on the real system.

### 2026-10-18: Phase 1 transactions roll back in-process only

- **Decision:** a failed install/remove/upgrade restores every file and the
  database (backup-then-rename journal, DB commit last). A run that is
  *killed* leaves `usr/lib/spk/staging/` behind and the next run refuses to
  start. No automatic crash recovery.
- **Why:** CLAUDE.md 6.2 makes system updates crash-safe through btrfs
  snapshots (Phase 3): the running system is never modified. A journal
  replayed after a crash would duplicate that mechanism.

### 2026-10-18: Signature domain separation and key format

- **Decision:** the signed message is `"spk-package-v1\0" || sha256(file)`
  or `"spk-index-v1\0" || sha256(file)`; key files are a one-line header
  plus base64 (no PKCS#8). Secret keys refuse to load if group/other can
  read them. `verify_strict` is used.
- **Alternatives:** signing raw bytes; minisign/signify formats; OpenPGP.
- **Why:** domain tags make a package signature useless as an index
  signature and vice versa. The format is a few lines of code with
  ed25519-dalek, no extra parser surface.

### 2026-10-18: Verify-then-extract via a private copy

- **Decision:** a package is copied into an unlinked temp file while being
  hashed; the signature and index checks run on that hash; extraction reads
  only the private copy.
- **Why:** removes the window where a file could be swapped between being
  verified and being unpacked.

### 2026-10-18: Phase 1 host toolchain

- **Decision:** until Phase 2 builds our own toolchain, `spk build` uses
  the Nix devShell's compilers: `/nix/store` is bound read-only into the
  sandbox and `PATH` is the devShell's `/nix/store` entries. Build
  dependencies go into a fresh sysroot mounted at `/usr`. `LDFLAGS` adds
  `-Wl,-rpath,/usr/lib` because the host loader does not search it.
- **Consequence:** Phase 1 packages link against the host glibc and are
  **not bootable**; they exercise the package pipeline only. Phase 2
  rebuilds everything against our own glibc.
- **Why:** spk has to exist before it can build the toolchain (CLAUDE.md
  6.3 drives the bootstrap through spk recipes).

### 2026-10-18: Build sandbox details

- **Decision:** bubblewrap with `--unshare-all` (network included),
  `--cap-drop ALL`, `--new-session`, `--die-with-parent`, a tmpfs root, and
  `--clearenv`. When spk runs as root it first drops to uid 65534 with
  `setpriv --clear-groups --inh-caps=-all --bounding-set=-all
  --no-new-privs`; the build sees uid 1000 inside its own user namespace.
  Source extraction also happens inside the sandbox. `SOURCE_DATE_EPOCH`
  is fixed at 315532800 (1980-01-01, the earliest ZIP date).
- **Why:** CLAUDE.md rule 7 (never run build scripts as root) and 6.2 (no
  network after source fetch). Unpacking untrusted tarballs is also code
  running on attacker-controlled input, so it gets the same sandbox.

### 2026-10-18: Build output trust boundary

- **Decision:** after the sandbox exits, spk inspects `DESTDIR` without
  following symlinks (`O_NOFOLLOW`, no-follow walks, a check that every
  directory component is real) before moving `etc/` or packaging anything.
- **Why:** spk runs as root on a tree the build user controlled; a planted
  `usr -> /` symlink must not redirect spk's own file operations.

### 2026-10-18: Recipe versions follow nixpkgs where possible

- **Decision:** the Phase 1 recipes use the versions nixpkgs pins in our
  flake lock, from upstream release tarballs.
- **Why:** an independent, reviewed sha256 for the same file. 7 of 10
  recipes (zlib, bzip2, xz, expat, libffi, pcre2, libxcrypt) match nixpkgs
  byte for byte; zstd matches upstream's published `.sha256`; lz4 and
  libarchive release tarballs are pinned trust-on-first-use (nixpkgs uses
  git archives for those). It also keeps us on
  current security releases (expat, xz).

### 2026-10-18: Seeding the source cache from Nix

- **Decision:** `tools/seed-sources-from-nix.sh` (`just seed-sources`)
  copies sources from the Nix binary cache into spk's source cache, only
  when nixpkgs pins the identical sha256.
- **Why:** the cloud dev container's proxy blocks some upstream hosts
  (sourceware.org, GNU mirrors, kernel.org). spk re-verifies every cached
  source against the recipe hash, so seeding cannot change a build.

### 2026-10-18: Split-package dependency pinning

- **Decision:** an unversioned runtime dependency on another package from
  the same recipe (e.g. `xz` -> `liblzma`) is pinned to the exact
  version-release being built.
- **Why:** tools and their libraries from one source must move together.

### 2026-10-18: Refuse `--root /` on non-Solace hosts

- **Decision:** `spk install/remove/upgrade/key trust` refuse to modify `/`
  unless `/usr/lib/os-release` says `ID=solace`.
- **Why:** during development spk runs on NixOS and in containers, where
  the default root would be the host itself (CLAUDE.md rule 8 spirit).

### 2026-10-19: Logo

- **Decision:** the Solace logo is a half-set sun over still water with its
  reflection as tapered ripples, on our n=5 superellipse plate with a dusk
  gradient (night indigo to horizon coral). A single-colour mark (half sun
  + three ripples) on a 4-unit grid serves the menu bar, boot splash, and
  GRUB theme. Both are generated from `tools/brand/gen-logo.py`.
- **Alternatives:** a koi fish (ties to the desktop name but competes with
  it, and reads poorly at 16 px); a monogram "S" (generic).
- **Why:** "Solace" = sol + calm; the image is calm, warm, and original
  (rule 9), survives reduction to a 16 px single-colour glyph, and its
  palette gives the design system a starting point. The human asked for
  the familiar desktop-icon look: squircle plate, rich gradient, soft
  depth.

### 2026-10-19: Glass logo variant

- **Decision:** added `solace-icon-glass.svg`, a translucent glass version
  of the logo on a transparent background, generated by the same script.
  It is called "glass" (and the icon mode "Clear"), never by the vendor's
  marketing name for this style (rule 9).
- **Why:** the human asked for it, and it is the reference look for the
  "Clear" adaptive-icon mode (CLAUDE.md 6.9: frosted-glass background,
  white foreground).

### 2026-10-19: Bootstrap design (Phase 2)

- **Decision:** the bootstrap follows Linux From Scratch chapters 5-8 as
  spk stages in `bootstrap/plan.toml`, run by `spk bootstrap`:
  1. `cross` (LFS 5): cross binutils + GCC (prefix `/usr/cross`), Linux
     headers, glibc, libstdc++, built with the host's compilers for target
     `x86_64-solace-linux-gnu` (deliberately not the host's triple, so
     nothing builds natively against the host by accident).
  2. `temp` (LFS 6): temporary tools cross-compiled for Solace.
  3. `native` stages (LFS 7-8): the sandbox contains **only** our own
     packages (no `/nix`), and the final system is built there.
- **Toolchain modes** `host` / `cross` / `native` are a recipe/stage
  property (docs/formats/bootstrap-plan.md). Inside the sandbox the
  target sysroot is mounted at `/usr`, so the cross tools use
  `--with-sysroot=/`; this also makes LFS's libtool edit unnecessary.
- **Temporary packages** are versioned `X~bootstrap` and keep the final
  package's name. `~` sorts first, so once `X` is built it replaces the
  temporary package in every later sysroot automatically; no special
  casing in the solver.
- **Layout:** libraries and the dynamic loader live in `/usr/lib`; the
  ELF interpreter stays the standard `/lib64/ld-linux-x86-64.so.2`
  (needed by NVIDIA userspace and Steam) and resolves through a
  `/lib64 -> usr/lib` link. `/usr/sbin` stays a real directory.
- **Recipe phases** each run in their own subshell from the source
  directory, so a `cd` in one phase cannot leak into the next.
- **Why:** CLAUDE.md 6.3 requires an LFS-style bootstrap driven entirely
  by spk recipes. Expressing it as ordinary signed packages means the
  bootstrap gets the same sandbox, verification, and rebuild-skipping as
  everything else.
- Bootstrap recipes (`bootstrap/recipes/`) never ship, so they need no
  justification lines; the final recipes that replace them do.

### 2026-10-19: Text login without PAM (Phase 2)

- **Decision:** Phase 2's text login is agetty (util-linux) + shadow's
  `login`, built without PAM. Linux-PAM arrives with the graphical greeter
  (Phase 9), when shadow and util-linux are rebuilt against it.
- **Why:** Linux-PAM 1.7 builds with Meson, which needs Meson and Ninja
  (and Python's packaging stack) in the base first. The locked stack still
  says Linux-PAM; this is only ordering.

### 2026-10-19: sched_ext deferred until the toolchain has pahole

- **Decision:** the Phase 2 kernel enables BPF and BPF JIT but not
  `CONFIG_SCHED_CLASS_EXT`, which requires BTF debug info and therefore
  `pahole` (dwarves, which needs CMake) at kernel build time.
- **When:** with the Performance settings (sched_ext profiles), after the
  dwarves and CMake recipes exist. Tracked in STATUS.md.

### 2026-10-19: Our own triple everywhere

- **Decision:** the final binutils and GCC are configured with
  `--build=x86_64-solace-linux-gnu`, so the system's compiler reports our
  triple (as the bootstrap compilers do) instead of `x86_64-pc-linux-gnu`.

### 2026-10-19: More trust-on-first-use sources

- **Decision:** `bc` 7.0.3 and `shadow` 4.20.2 are pinned from their
  official GitHub release assets, trust-on-first-use (nixpkgs packages a
  different bc and builds shadow from a git archive). Every other final
  source matches nixpkgs' pinned hash byte for byte.

### 2026-10-19: Our Rust tools are built with the host's rustc, linked by us

- **Decision:** `toolchain = "rust"` builds run in the native sandbox
  (our gcc, binutils, glibc) with only `cargo`/`rustc` taken from the
  host, until Rust itself is bootstrapped. Crates are vendored before the
  sandbox (cargo checks each against Cargo.lock) and the build runs
  offline. Outputs are static binaries linked by our gcc against our
  glibc, so nothing from the host remains in them.
- **Why:** bootstrapping rustc is a large project of its own (it needs
  LLVM and a stage-0 compiler); the binaries we ship are independent of it.

### 2026-10-19: The image builder needs no root and no loop devices

- **Decision:** `image/build.sh` installs packages into a staging root,
  lays out the subvolumes as directories, and lets `mkfs.btrfs --rootdir
  --subvol ro:@snapshots/1 ...` write the filesystem straight into a file;
  the ESP is `mkfs.fat` + `mcopy`, the disk is `sfdisk` + `dd` on a file.
  It re-executes itself in a user namespace (`unshare --map-root-user`) so
  files are root-owned in the image without real root.
- **Alternatives:** loop mounts + `btrfs subvolume` (needs root and
  touches the host's block layer); fakeroot (cannot see raw syscalls).
- **Why:** packages only ever carry root-owned files, so a single-uid
  namespace is enough, and nothing can reach a real disk (rule 8).

### 2026-10-19: Dev images: initramfs on the ESP, dev root password

- **Decision:** the initramfs is generated from the packaged sol-init by
  the packaged `sol-mkinitramfs` and goes only to the ESP (via
  sol-bootgen), not into `@snapshots/<id>`, so every file in the sealed
  system still comes from a package. Phase 3 transactions regenerate it
  per snapshot. QEMU images set a root password (`solace`, override with
  `SOL_ROOT_PASSWORD`) in `@etc`; installs (Phase 12) create real users.

### 2026-10-19: Temporary GRUB binary from the workbench

- **Decision:** until `recipes/grub` can fetch the GRUB 2.14 release
  tarball (GNU mirrors are blocked in the cloud build environment, and
  nixpkgs builds GRUB from git + gnulib, also blocked), `image/build.sh`
  makes `BOOTX64.EFI` with the dev shell's `grub-mkimage` and records
  `grub = "dev-workbench (not shippable)"` in `build/image/image.toml`.
  Theme, fonts, and grub.cfg are ours. This is a known gap, listed in
  STATUS.md, not a design choice.
