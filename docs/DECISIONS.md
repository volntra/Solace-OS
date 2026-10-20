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
- `bzip2`: bzip2 tool and libbz2; libarchive and pcre2grep read .bz2, and many upstream sources ship as .tar.bz2.
- `coreutils`: the basic userland (ls, cp, ...).
- `diffutils`: diff/cmp, needed by most builds.
- `elfutils`: libelf, required by the kernel's objtool.
- `expat`: XML parser required by dbus-daemon, fontconfig, Mesa, and wayland-scanner.
- `file`: libmagic, used by builds and later by Files for type detection.
- `findutils`: find/xargs, needed by builds and scripts.
- `flex`: lexer generator, needed to build the kernel and others.
- `gawk`: awk, needed by builds (glibc, kernel).
- `gcc`: the compiler (C, C++) plus gcc-libs (libgcc_s, libstdc++) at runtime.
- `glibc`: the C library (locked stack: NVIDIA userspace and Steam need it).
- `gmp`: GCC's arithmetic library.
- `gperf`: perfect-hash generator; udev's build needs it.
- `grep`: needed by builds and scripts.
- `grub`: the boot loader on the ESP (locked stack); built for UEFI only, branding patched out.
- `gzip`: .gz archives (kernel image, sources).
- `less`: pager for text output (logs, man pages).
- `libarchive`: archive library Flatpak requires (it unpacks OCI/bundle layers); provides bsdtar.
- `libcap`: POSIX capabilities; required by udev, later by fern-init sandboxing and elogind.
- `libdrm`: kernel DRM interface; Mesa and the compositor's DRM backend use it.
- `libevdev`: evdev device wrapper; required by libinput.
- `libffi`: required by wayland-client/server and GLib (needed for the Wayland stack and Flatpak).
- `libglvnd`: GL vendor dispatch (libEGL, libGLESv2): Mesa and NVIDIA's driver coexist through it.
- `libinput`: input devices (pointer acceleration, touchpads) for fern-comp.
- `libxcrypt`: crypt(3) with yescrypt for Linux-PAM and shadow password hashing.
- `libxkbcommon`: keymaps and keyboard state for fern-comp and every Wayland client.
- `linux`: the kernel, with our configuration (boot/kernel/fern.config); its `linux-build` split is the tree out-of-tree modules (NVIDIA) compile against.
- `linux-headers`: kernel API headers for glibc and everything else.
- `lz4`: fast compression for zram swap tooling, libarchive, and kernel/initramfs images.
- `m4`: macro processor needed by autoconf-generated builds and bison.
- `make`: build driver.
- `mesa`: open GL ES/EGL/GBM: virgl for QEMU, softpipe as fallback; hardware drivers later.
- `meson`: build system of the graphics stack (Mesa, Wayland, libinput, udev).
- `mpc`: GCC's complex-arithmetic library.
- `mpfr`: GCC's floating-point library.
- `mtdev`: multitouch protocol translation; required by libinput.
- `ncurses`: terminal handling for bash, readline, util-linux, less.
- `ninja`: build executor for meson and cmake builds.
- `nvidia`: NVIDIA's driver (open kernel modules, GSP firmware, EGL/GBM userspace); optional, offered by the installer for NVIDIA GPUs.
- `openssl`: TLS/crypto; the kernel's certificate tool needs libcrypto, and so will networking.
- `patch`: applying recipe patches.
- `pcre2`: regular expressions for grep, GLib, and Qt.
- `pkgconf`: pkg-config for builds (shadow needs it; so will Mesa, Wayland, Qt).
- `python`: the final Python 3 (LFS 8.x), with zlib/ssl/...; runs meson and Mesa's and udev's build scripts.
- `python-jinja2`: templates in udev's (systemd) build.
- `python-mako`: code generation in Mesa's build.
- `python-markupsafe`: required by Jinja2 and Mako.
- `python-packaging`: version parsing in Mesa's build (Python 3.14 has no distutils).
- `python-pyyaml`: YAML parsing in Mesa's build.
- `readline`: line editing for bash and gawk.
- `seatd`: libseat: lets fern-comp open DRM and input devices without root; seatd until elogind arrives.
- `sed`: needed by builds and scripts.
- `shadow`: user/group/password tools and the text-console login.
- `fern-bootgen`: ours (boot/bootgen/): grub.cfg, ESP kernels, initramfs.
- `fern-init`: ours (init/): PID 1, service manager, fernctl, fern-log.
- `fern-base`: ours (image/base/): os-release, factory /etc, console units.
- `fern-grub-theme`: ours (boot/grub-theme/): boot menu theme + Inter fonts for GRUB.
- `fern-comp`: our compositor (Phase 4); the image's display server.
- `fern-wallpapers`: our wallpapers, rendered from assets/wallpapers.
- `inter`: the UI typeface (CLAUDE.md 3); the core draws window titles with it.
- `spk`: ours (pkg/): the package manager; system transactions on a running Fern.
- `tar`: unpacking sources; archives.
- `udev`: device manager from the systemd tree (standalone, locked stack): hotplug, device nodes, libudev for libinput.
- `util-linux`: mount, agetty, fdisk, lsblk, and libmount/libblkid.
- `wayland`: the Wayland libraries and wayland-scanner: the protocol of the whole desktop.
- `wayland-protocols`: protocol extensions (xdg-shell, ...) used by fern-comp, Mesa and clients.
- `xkeyboard-config`: keyboard layout data for libxkbcommon.
- `xz`: liblzma and the xz tool; kernel module/initramfs compression, libarchive, and .tar.xz sources.
- `zlib`: deflate; required by almost everything (libpng, Qt, kernel tools, libarchive).
- `zstd`: our package format (.fernpkg is tar.zst), btrfs compression tooling, and initramfs compression.

## Log

### 2026-10-18: Names

- **Decision:** OS = **Solace**, prefix = `sol` (D-Bus `org.sol.*`, config
  paths `/etc/sol`, `~/.config/sol`), package manager = `spk`, shell = `ace`,
  desktop umbrella = **Koi**. (Superseded on 2026-10-20: the OS is now
  **Fern**, prefix `fern`; see that entry.)
- **Interpretation to confirm:** for the desktop name the human answered
  "SolOS 1: Koi". Read as: the desktop is called **Koi**, and
  "SolOS 1: Koi" is the release label for version 1 (OS name stays Solace,
  as chosen in the first answer). If "SolOS" was meant to replace "Solace"
  as the OS name, that is a search-and-replace in CLAUDE.md and the docs
  today; it gets more expensive once branding art exists.
- **Alternatives offered:** Lumen, Halcyon, Orbit; shell names calm, hush,
  solsh, ember; desktop names Harbor, Meadow, Glow, Sanctum.

### 2026-10-18: One Cargo workspace at the repo root

- **Decision:** all Rust crates (spk, fern-init, fern-bootgen, fern-comp, ...)
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

### 2026-10-18: `tools/` directory and `ferncheck`

- **Decision:** added `tools/` (not in the brief's layout) holding
  `ferncheck`, a std-only Rust binary run by `just lint`. It checks: no
  unfilled name placeholders anywhere; no Apple trademarked names in shipped
  paths (`apps/ assets/ boot/ bootstrap/ de/ image/ init/ pkg/ recipes/
  sh/`) unless the line says `ferncheck: allow-name`; no Apple font or icon
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

- **Decision:** `packages.ferncheck` and `checks.ferncheck` build and test
  ferncheck with `rustPlatform.buildRustPackage`; `just nix-check` runs
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

### 2026-10-18: Refuse `--root /` on non-Fern hosts

- **Decision:** `spk install/remove/upgrade/key trust` refuse to modify `/`
  unless `/usr/lib/os-release` says `ID=fern`.
- **Why:** during development spk runs on NixOS and in containers, where
  the default root would be the host itself (CLAUDE.md rule 8 spirit).

### 2026-10-19: Logo

- **Decision:** the Fern logo is a half-set sun over still water with its
  reflection as tapered ripples, on our n=5 superellipse plate with a dusk
  gradient (night indigo to horizon coral). A single-colour mark (half sun
  + three ripples) on a 4-unit grid serves the menu bar, boot splash, and
  GRUB theme. Both are generated from `tools/brand/gen-logo.py`.
- **Alternatives:** a koi fish (ties to the desktop name but competes with
  it, and reads poorly at 16 px); a monogram "S" (generic).
- **Why:** "Fern" = fern + calm; the image is calm, warm, and original
  (rule 9), survives reduction to a 16 px single-colour glyph, and its
  palette gives the design system a starting point. The human asked for
  the familiar desktop-icon look: squircle plate, rich gradient, soft
  depth.

### 2026-10-19: Glass logo variant

- **Decision:** added `fern-icon-glass.svg`, a translucent glass version
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
  2. `temp` (LFS 6): temporary tools cross-compiled for Fern.
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
  different bc and builds shadow from a git archive). `pkgconf` 3.0.10
  (nixpkgs builds a git archive) is the release tarball, its hash
  cross-checked against Alpine's pinned sha512; `grub` is checked against
  its GNU signature. Every other final source matches nixpkgs' pinned
  hash byte for byte.

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

- **Decision:** the initramfs is generated from the packaged fern-init by
  the packaged `fern-mkinitramfs` and goes only to the ESP (via
  fern-bootgen), not into `@snapshots/<id>`, so every file in the sealed
  system still comes from a package. Phase 3 transactions regenerate it
  per snapshot. QEMU images set a root password (`fern`, override with
  `FERN_ROOT_PASSWORD`) in `@etc`; installs (Phase 12) create real users.

### 2026-10-19: GRUB built by our recipe

- **Decision:** `recipes/grub` builds GRUB 2.14 from the signed GNU
  release (signature checked against the GNU keyring before pinning the
  hash) and ships `usr/lib/fern/boot/grubx64.efi`; the image builder copies
  it to `EFI/BOOT/BOOTX64.EFI` and refuses to build without it. A
  workbench fallback existed briefly while GNU mirrors were blocked in the
  cloud environment; it was removed once network access was opened.

### 2026-10-19: The system stage's base set is pinned to the bootstrap repo

- **Decision:** `spk bootstrap` pins each base package of a stage with
  dependency repos to the exact version found there (for the system stage:
  the `~bootstrap` tools). Final packages see each other only through
  their explicit build deps.
- **Why:** with "newest wins", a package's sysroot changed as the stage
  built final versions of its base tools, so every rerun rebuilt
  everything before the last change, and finishing gcc would have forced
  a full rebuild. Pinning makes each package's inputs independent of
  build history.

### 2026-10-19: gawk carries upstream's 5.4.1 fix; everything after it rebuilt

- **Decision:** both gawk recipes apply upstream commit bf85f8a3 (the
  NODE layout fix for builds without MPFR/GMP). Without it, gawk 5.4.1
  prints an uninitialized value as "0" after a numeric comparison
  (`BEGIN { if (a[1] == 0) {}; print a[1] "z" }` prints `0z`), which broke
  GCC's generated options.cc.
- **Why rebuild everything after it:** the broken gawk was in every
  chroot- and system-stage sandbox, and glibc, the kernel and others
  generate code with awk. Patching only gcc's awk would leave packages
  built by a broken tool in the system.

### 2026-10-19: System transactions (Phase 3)

- **Decision:** an update snapshots the **booted** system (not the menu's
  default), bind-mounts the copy as `usr` of a staging root under
  `/run/fern/tx`, and runs the ordinary package transaction there with
  the running system's trusted keys. The initramfs trigger, metadata
  (`/usr/lib/fern/system.toml`), seal (read-only flag) and fern-bootgen
  follow; any failure before the boot menu is written deletes the
  snapshot. An unsealed snapshot can only be an interrupted transaction
  and is deleted by the next one. Rollback sets `default` in
  `/etc/fern/boot.toml` and reruns fern-bootgen. Spec:
  docs/formats/system.md.
- **Alternatives:** ostree-style deployments (a second store and a
  checkout step, more than we need with btrfs snapshots); updating
  `/usr` in place with a journal (the running system would change).
- **Why:** btrfs snapshots are cheap and atomic, the existing package
  transaction is reused unchanged, and the running system and boot menu
  are never half-updated.
- **Details:** fern-init mounts the volume's top level at
  `/run/fern/btrfs` (root-only, `noexec`) and the ESP at `/boot` (found
  by `fern.esp=UUID=`, which fern-bootgen now writes). Because the top
  level is `noexec`, the initramfs is written with fern-bootgen's library
  (from the new system's packaged fern-init), not by running a tool from
  the snapshot. Package triggers (`[triggers]`) are refused until they
  are implemented. `SPK_TEST_PAUSE=before-seal` pauses a transaction for
  the QEMU test's power cut (CLAUDE.md section 8); it does nothing unset.

### 2026-10-19: `spk build` accepts dependencies already packaged

- **Decision:** a build may depend on packages that no recipe in its
  recipes directory produces if a `--dep-repo` has them (names read from
  the signed index). Needed to rebuild one recipe outside the full set
  (the Phase 3 demo update).

### 2026-10-19: Compositor foundation (Phase 4, stage 1)

- **Decision:** fern-comp is built on Smithay 0.7.0 (the locked stack), with
  only the features in use (`wayland_frontend`, `desktop`,
  `backend_winit` for nested development; DRM/libinput/libseat come with
  the DRM backend). Input routing and the move/resize grabs are adapted
  from Smithay's `smallvil` example (MIT; attribution in each file), with
  every `unwrap` in event handling replaced by early returns: a panicking
  compositor takes every app down with it.
- **Animation:** `spring.rs`, closed-form damped harmonic oscillator
  (frame-rate independent), presets from the HIG (critically damped).
- **Development without a GPU:** the dev shell carries Xvfb and Mesa; the
  cloud container runs fern-comp nested in Xvfb on Mesa's llvmpipe (GLES
  3.2). EGL is not pointed at Mesa globally (`MESA_EGL_VENDOR_DIR` is only
  used by tests), so on the NixOS host the nested compositor uses the real
  driver (NVIDIA).

### 2026-10-19: Our renderer (Phase 4, stage 2)

- **Decision:** windows, shadows and the wallpaper are Smithay render
  elements (`de/comp/src/render.rs`), drawn through
  `OutputDamageTracker`, so damage tracking is Smithay's (proven) and our
  effects stay small shaders.
- **Rounded corners:** a custom texture shader replaces the default one
  while a window's surfaces draw; it clips to the window's rounded
  rectangle with a signed-distance function (one pixel of antialiasing).
  Clipping happens in output pixels (`gl_FragCoord`), so subsurfaces
  clip to the same rectangle as their parent. Rounded windows report no
  opaque regions and are never scanned out directly.
- **Shadows:** analytic Gaussian blur of a rounded rectangle (erf along
  x, four samples along y; Evan Wallace's method), not a blurred texture:
  no offscreen passes, resolution-independent. Masked out under the
  window so translucent windows do not darken. Focused windows get the
  larger, darker shadow. The element is cached per window, so an
  unchanged shadow costs no damage.
  Alternatives: nine-patch shadow textures (cheap, but fixed radii and
  sizes); a real blur pass per window (expensive).
- **Wallpaper:** PNG decoded once (`png` crate), drawn object-fit cover.
  Found at `$FERN_WALLPAPER`, `/usr/share/fern/wallpapers/dusk.png`, or
  `build/wallpapers/dusk.png`. A missing wallpaper falls back to the
  plain background colour; it is not an error.
- **New windows take focus** (raised and activated), as users expect.
  The window manager (Phase 5) will own this policy.

### 2026-10-19: Background blur (Phase 4, stage 2)

- **Protocol:** clients ask for blur with KDE's `org_kde_kwin_blur`
  (whole surface or a region, double-buffered with the surface). Qt and
  KDE apps already speak it, and our own shell (Qt) will use it for its
  materials. Alternatives: a private protocol (nothing else would use
  it); `ext-background-effect-v1` (still a draft; add it when it lands).
- **Algorithm:** dual-Kawase (Bjørge, ARM 2015), 3 passes with offset 4
  (reach ~64 px), then a 1.35 saturation boost and light dithering on
  the last pass (the HIG's "blur + tint + noise"; the tint is the
  client's own translucent fill). Chosen over a separable Gaussian: the
  same visual reach at a fraction of the texture reads, which is what
  keeps the dock blur cheap at 165 Hz.
- **Cost control:** only the blurred area plus the reach is rendered
  offscreen, and the result is reused until an element behind it changes
  (keyed by element id, commit and position). A window's own shadow is
  not part of its backdrop.
- **Shadows** update their uniforms only when the window's size or focus
  style changes: updating them every frame counted as damage and forced a
  redraw (and a re-blur) of every frame.
- **Nested pacing:** with no damage nothing is presented, and the next
  frame is checked a 60 Hz frame later rather than immediately.
- **Test client:** `fern-testclient` (Rust, wayland-client), the seed of
  stage 3's protocol and screenshot tests.

### 2026-10-19: Compositor tests: headless backend and pixel checks (Phase 4, stage 3)

- **Decision:** fern-comp has a headless backend (offscreen texture, Mesa's
  surfaceless EGL platform, llvmpipe forced) and an integration test that
  starts a compositor per scenario, connects real `fern-testclient`
  processes over Wayland, and asserts on pixels of read-back frames.
  Software rendering keeps the expected pixels identical on every machine,
  including the NVIDIA host.
- **Assertions on pixels, not golden images:** each check names what it
  tests (corner pixel cut, edge pixel solid, shadow darker below the
  window, spread of colours under the blur, average colour of the blurred
  stripes). Golden images would break on every intended visual tweak and
  say nothing about what went wrong.
- **Own `main` (`harness = false`):** scenarios share process state
  (EGL, the environment: `XDG_RUNTIME_DIR`, the Mesa vendor file), which
  must be set before any thread exists; running them one at a time in a
  single thread makes that sound.
- **fern-testclient moved into the fern-comp package** so the test finds it
  through `CARGO_BIN_EXE_fern-testclient`.
- **Not done:** protocol tests for protocols we do not have yet; frame
  time measurements (need real hardware).

### 2026-10-19: The graphics base (Phase 4, stage 4)

- **Decision:** the stack under fern-comp is built by our recipes in the
  system stage: Python (final), ninja, meson and the Python modules their
  builds need, gperf, libcap, udev, libdrm, wayland, wayland-protocols,
  xkeyboard-config, libxkbcommon, libevdev, mtdev, libinput, seatd,
  libglvnd and Mesa. spk gains a `meson` build preset (subprojects never
  downloaded: every dependency is a recipe).
- **Mesa without LLVM:** Wayland platform, EGL + GBM + GLES2 through
  libglvnd, two drivers: virgl (QEMU virtio-gpu with host 3D) and softpipe
  (software fallback). LLVM would add hours of build time for llvmpipe and
  radeonsi only; it comes with the hardware that needs it. NVIDIA uses its
  own userspace through the same libglvnd.
- **udev from the systemd tree** (locked stack), as Gentoo's systemd-utils
  does: configure systemd with everything but udev off, install to a
  staging dir, keep udev. Without acl (seat ACLs, elogind's job later) and
  kmod (module autoloading) for now.
- **seatd/libseat until elogind:** smithay opens devices through libseat;
  its seatd and builtin backends work now, the logind backend is enabled
  when elogind is built (Phase 9 needs it for the greeter).
- **Final Python:** the bootstrap Python (LFS ch. 7) has no zlib, so meson
  cannot run on it. The system stage builds Python 3.14 against our
  libraries, and recipes that need it name `python` as a build dependency
  (spk swaps in a stage's own copy of a base package only when a recipe
  names it, so existing packages are not rebuilt).
- **Sources and GitHub archives:** every new source is pinned by sha256
  and cross-checked against a second record (Gentoo's Manifest SHA-512,
  PyPI's digests, or upstream's sha256sum). Where only GitHub tag archives
  exist (ninja, libxkbcommon, systemd, and sourcehut's seatd), the recipe
  keeps the upstream URL and lists Gentoo's distfiles copy (the identical
  file) as a mirror. Mesa 26.2.4 and libevdev 1.13.7 rather than the
  newest point releases, which no second source had yet.
- **Rust recipes include `de/comp/Cargo.toml`:** cargo loads every
  workspace member; only the manifest is included so compositor changes
  do not rebuild fern-init, spk or fern-bootgen.

### 2026-10-19: fern-comp on the display (Phase 4, stage 5)

- **Decision:** a DRM backend modelled on Smithay's anvil, reduced to one
  GPU: libseat session, udev for GPU discovery and hotplug, DRM + GBM +
  EGL, `DrmOutputManager` per connector, libinput for input. Each display
  runs its own loop: render, queue, wait for vblank; an empty frame is
  skipped and checked again one refresh period later.
- **GPU choice:** the boot GPU unless it is the firmware's framebuffer
  (simpledrm), then any real GPU. simpledrm stays next to the real driver
  in QEMU and on some PCs (NVIDIA among them), and drawing to it shows
  nothing once the real driver runs. Found because the QEMU test looked at
  the wrong screen.
- **Cargo features:** `nested` (winit) and `drm`; the image's package is
  built with `drm` only. fern-comp links dynamically (the rust toolchain's
  static default cannot load EGL/GBM drivers).
- **The pointer** is ours: an arrow drawn in code (no image assets),
  placed on the cursor plane by Smithay when the hardware has one.
- **Session keys:** Ctrl+Alt+F1..F12 switch VTs through libseat,
  Ctrl+Alt+Backspace quits the compositor.
- **Services ship with their packages:** udev (hwdb on first boot into
  /etc, udevd, coldplug trigger + settle), seatd, fern-comp; each recipe
  installs its unit from its recipe directory. A test parses every unit in
  the repository with fern-init's parser.
- **tty1 is the desktop**, the console login moved to tty2. fern-comp runs
  as root through seatd until the login screen (Phase 9) starts it per
  user.
- **QEMU:** one GPU (`-vga none -device virtio-vga`) plus virtio keyboard
  and tablet for `--screen` and windowed runs; the headless mode keeps
  QEMU's default VGA for the boot-menu screenshots.
- **Wallpapers** are a package (`fern-wallpapers`), rendered from our
  SVGs with resvg from the dev workbench until resvg has a recipe.
- **Standard groups** added to the factory /etc/group (render, kvm, lp,
  cdrom, ...): udev's rules assign device nodes to them.

### 2026-10-19: NVIDIA, GPU clients and kernel modules (Phase 4, stage 6)

- **Driver:** NVIDIA 595.104.02 (the production branch Gentoo marks
  stable; supports kernels up to 7.3 and Blackwell), open kernel modules
  (the only ones for the RTX 50 series). One source, NVIDIA's .run, which
  carries the open modules' source; cross-checked against Gentoo's
  Manifest. Packaged as `nvidia` (userspace) and `nvidia-kmod` (modules
  for exactly linux 7.2-3, GSP firmware, the module list, device nodes).
  Not in the base image: the installer offers it on NVIDIA hardware. The
  userspace is NVIDIA's binary, redistributed unmodified (as distributions
  do). Vulkan, CUDA, VDPAU and NVENC are left out for now.
- **Device nodes:** NVIDIA's modules register /dev/nvidia* without a
  device class, so a oneshot unit creates them (group video) instead of
  the setuid nvidia-modprobe (spk packages carry no setuid files).
- **Kernel modules without kmod:** fern-init loads the modules packages
  list (`/usr/lib/fern/modules.d/*.conf`, overridable in /etc) with
  `finit_module`, letting the kernel decompress `.ko.zst`
  (`CONFIG_MODULE_DECOMPRESS`). No depmod, no dependency resolution (a list
  orders its modules), no hotplug autoloading. Alternatives: kmod (another
  package and a second module configuration format, for features we do not
  use yet). zram became a module so every QEMU boot exercises the loader.
- **linux-build:** the kernel recipe also ships the build tree out-of-tree
  modules need (as distributions' headers packages), depending on elfutils
  for objtool.
- **GPU clients:** linux-dmabuf with the renderer's formats and the GPU's
  render node as feedback; each buffer is test-imported before it is
  accepted (the backend owns the renderer and provides the check);
  linux-drm-syncobj when the kernel driver supports syncobj eventfds. A
  commit with a GPU buffer waits on its explicit acquire point, or else
  on the buffer's implicit fence, before it is applied.
- **Measuring on the real display:** `FERN_COMP_STATS=1` makes the DRM
  backend report frames per second, frame intervals and missed refreshes
  from the kernel's presentation timestamps; `just comp-drm` runs it on
  the host's display with an animated test window (double-buffered, one
  frame per frame callback), so the 165 Hz target can be checked on the
  human's machine before Fern itself runs there.


### 2026-10-20: Event-driven redraws on the real display (Phase 4)

First hardware run on the RTX 5070 Ti: 160 fps against a 165 Hz display.
After a frame with no damage the DRM loop slept a full refresh on a timer
before trying again, so a client commit that landed just after an empty
frame missed the next vblank (about 5 a second).

- **Decision:** each display is in one of four states (stopped, a frame
  scheduled, a frame queued for vblank, idle). A client commit or an
  input event pings the loop, which draws every idle display at once; a
  queued display draws again on its vblank. Frame callbacks are still
  paced to the refresh while nothing changes, so idle clients do not spin.
- **Client flush:** replies to clients are flushed after every event-loop
  wake-up, not only after a frame, since an idle display draws nothing and
  a new window would otherwise wait forever for its configure (caught by
  `just qemu-test-comp`).
- **Alternatives:** a shorter timer (still adds latency and wakes an idle
  machine); always rendering every vblank (wastes power and defeats damage
  tracking).
- **Result:** on the RTX 5070 Ti (DP-1, 180 Hz preferred mode) one
  animated window went from 160 to 180.1 fps with 0 missed refreshes.
  fern-comp's logs now go to stderr, uncoloured unless it is a terminal (they
  went to stdout, so `just comp-drm` kept an empty log file).
  Under the full 6.5 load (three animated windows plus a dock-sized
  glass panel re-blurring them) it held 180.2 fps with 0 missed refreshes:
  Phase 4's hardware milestone. Frame timing restarts when the session
  pauses, so a report never mixes in time spent on another VT.

### 2026-10-20: Solace becomes Fern

- **Decision (the human's):** the OS is renamed **Fern**, prefix `fern`
  (binaries `fern-*`, `fernctl`, D-Bus `org.fern.*`, paths `/etc/fern`,
  `/usr/lib/fern`, `/run/fern`, `/boot/fern/<id>`, kernel parameters
  `fern.root=` etc., file types `.fernpkg`, `.fernapp`, `.fernicon`,
  kernel release `-fern`). `spk`, `ace` and **Koi** stay; the version 1
  label is "Fern 1: Koi". New identity: the logo is a simple pine tree (a
  soft cone with two branch cuts, chosen from four sketches), the palette
  is forest green and near-black/off-white (docs/design/HIG.md, Brand),
  and the default wallpaper is **Grove**, replacing Dusk. At the human's
  request Grove aims for a photograph rather than a flat drawing (spruce
  ridges in textured fog, light shafts, grain), and the boot menu keeps its
  centred layout with GRUB-style detail: a heading, the list on a frosted
  panel, a green selection bar, a scrollbar, a countdown bar, and a line
  of keys including E (edit) and C (command line), which work because the
  menu has no password.
- **Why:** "Solace" is an existing software company whose appliance OS is
  called SolOS, a direct clash with the name and the "SolOS 1: Koi" label.
  Fern clashes with no operating system; the nearest is Feren OS (a Linux
  distribution, sometimes misspelled "Fern OS"), so the name is used alone,
  never "Fern OS". Names searched and set aside: Vesper (Vesper Linux),
  Lyra (Lyra OS), Vela (openvela), Kumo (kumaOS), Moss (a Rust kernel),
  Ember, Dusk, Horizon (existing OSes).
- **Kept on purpose:** the toolchain's target triple
  `x86_64-solace-linux-gnu`, the `$SOL_TGT` variable its recipes use (spk
  exports it beside `$FERN_TGT`) and their package summaries. Rewording
  those recipes changes their build stamps and rebuilds the whole
  toolchain (hours); they change with the next toolchain update. Neither
  is visible outside a compiler's `-dumpmachine`. The GitHub repository is
  still `volntra/Solace-OS` until the human renames it.

### 2026-10-20: The window manager as a process (Phase 5, stage 1)

- **Decision:** fern-wm is its own process speaking a private Wayland
  protocol, `fern_wm_v1` (`de/protocols/fern-wm-v1.xml`, spec
  docs/protocols/wm.md), with integer ids for windows and outputs and
  every request applied at `commit`. The core starts it on a socket pair
  (stdin, `WAYLAND_SOCKET=0`), so no other client can reach the global,
  and restarts it after a crash (300 ms, at most 5 times in 30 s). On bind
  the core re-describes the whole desktop including what the previous
  window manager stored with `save`/`save_wm`: that is the whole crash
  recovery contract, and nothing on screen moves meanwhile. Without a
  window manager the core falls back to its own placement and
  click-to-focus.
- **Policy is a pure function:** `de/wm/src/policy.rs` turns events into
  commands with no Wayland in it, so it is unit tested directly; the
  compositor's pixel tests run the real fern-wm on a thread over a socket
  pair (placement, focus, and a kill-and-restart that must leave windows
  in place).
- **Bindings generated, not hand-written:** `wayland-scanner` generates
  both sides from the one XML (`fern-wm-proto`, features `server` and
  `client`).
- **Alternatives:** a WM library linked into the compositor (simplest,
  but a crash in policy code takes every app down, which 6.5 forbids);
  D-Bus or a custom socket format (a second IPC stack; Wayland already
  gives us typed messages, fd passing and the event loop integration).
- **Packaging:** fern-wm ships in the `fern-comp` package next to
  `/usr/bin/fern-comp`, which finds it there.

### 2026-10-20: Springs live in the core (Phase 5, stage 2)

- **Decision:** the window manager sends targets (`place`: position,
  scale, alpha, and a spring preset); the core animates them with
  `spring.rs` (closed form, frame-rate independent) and draws each window
  at its place plus the spring's remaining offset. Input always uses the
  target, so a click lands where the window is going. Scaling uses
  Smithay's rescale element around the drawn origin, with the rounded clip,
  radius and shadow scaled to match; blur behind a window waits until it
  stops moving (the backdrop cache is keyed on its position).
- **Why in the core:** frames are made there; the window manager would
  otherwise have to stream positions every frame over a socket, and a
  slow or crashed window manager would stall motion.
- **Fade-out:** `visible(false)` with an alpha-0 non-instant `place` keeps
  the window drawn (not hit) until the fade ends, so minimize and Spaces
  can animate out without the window manager timing anything.
- **Not done:** closing windows cannot fade (the client destroys its
  buffers); that needs a snapshot of the last frame, later.

### 2026-10-20: Titlebars are drawn by the core (Phase 5, stage 3)

- **Decision:** fern-comp answers every xdg-decoration request with
  server-side, and draws titlebars itself: a rounded rectangle and circle
  shader (`SHAPE_FRAG`) for the bar and the traffic lights, and Inter
  rasterized with `ab_glyph` for titles and glyphs, cached per window so an
  idle desktop damages nothing. The window manager decides *whether* a
  window has a titlebar (`decoration`) and what clicks on it do; the core
  reports which part was hit (`pointer_button.part`, with resize `edges`).
- **Alternatives:** the window manager draws decorations as its own
  surfaces (one more buffer per window, and a crashed WM would leave
  windows bare); clients draw their own (Qt and GTK would each look
  different, contrary to CLAUDE.md 6.6).
- **Why:** decorations stay on screen through a WM crash, move with the
  window's spring for free, and look the same for every toolkit. Clients
  that insist on drawing their own (no xdg-decoration) are still placed
  and managed; they simply get no titlebar.
- **Inter** becomes a package (`inter`, upstream release zip, SIL OFL);
  without it the core still runs and draws titlebars without titles.
- **Not done:** zoom fills the whole display until the shell reserves the
  menu bar and dock (Phase 6); minimize targets the bottom centre until
  the dock reports icon rectangles; dark-mode titlebars arrive with
  settingsd.

### 2026-10-20: Edge and corner tiling (Phase 5, stage 4)

- **Decision:** fern-wm owns tiling. During a titlebar drag it watches
  `move_update`; the pointer within 4 px of the left or right edge snaps
  to that half, within 96 px of a corner along an edge to the quarter, at
  the top edge (away from corners) to filling the screen. The bottom edge
  does nothing: it belongs to the dock. Tiles keep 8 px gaps (HIG:
  Spacing) and report all four `tiled` states so clients drop their own
  shadows. Super+arrows do the same from the keyboard.
- **Preview:** an overlay rectangle in Leaf at 25 % with the window
  radius, drawn *under* the dragged window via a new request,
  `overlay_beneath` (macOS does the same; drawn on top it tinted the
  window in hand).
- **Dragging out of a tile:** the window manager only asks for the old
  size; the core's move keeps the pointer at the same fraction across the
  titlebar as the width changes. Doing it in the window manager (place
  plus a new move) lagged the pointer by one round trip and the window
  jumped.
- **Crash memory:** what the window manager knows about a window that the
  core does not (its frame before zoom or tiling, its tile, minimized) is
  stored with `save` as `key=value;...` and read back on restart.
- **Alternatives:** automatic tiling layouts (not the macOS feel; floating
  stays the default per CLAUDE.md 6.6); snapping by window position
  instead of pointer position (harder to aim, and windows can be wider
  than the gap to the edge).
