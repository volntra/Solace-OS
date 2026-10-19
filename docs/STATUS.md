# Status

_Last updated: 2026-10-19 (session 2)_

Repository: https://github.com/volntra/Solace-OS (`main`).

## Current phase: 4 (compositor core): in progress

Stages: 1 foundation ✓, 2 renderer ✓ (corners, shadows, wallpaper,
background blur), 3 headless backend + pixel tests ✓, 4
graphics base recipes (Mesa, libdrm, libinput, ...), 5 DRM backend on
the QEMU image, then 165 Hz on the NVIDIA machine.

**Try it (nested in your desktop):**

```sh
nix develop
just wallpapers            # build/wallpapers/dusk.png, the default wallpaper
just comp weston-terminal  # any Wayland client; more with WAYLAND_DISPLAY=wayland-N
just comp-demo             # a striped window behind a frosted-glass one
just comp-test             # pixel tests, headless (also run by `just check`)
```

![sol-comp nested in Xvfb: wallpaper, rounded corners, shadows](design/img/comp-nested.png)
![just comp-demo: background blur under a translucent window](design/img/comp-blur.png)

What works: xdg-shell windows, placement (centred, cascading), focus on
open and on click, move/resize grabs, rounded corners (12 px, antialiased
in the shader), focus-dependent soft shadows (analytic Gaussian), the
wallpaper (cover-cropped), background blur for windows that ask for it
(`org_kde_kwin_blur`; dual-Kawase, cached until what is behind changes),
damage tracking (only changed regions are redrawn and presented; an idle
nested session uses almost no CPU). `sol-testclient` (de/comp/src/bin)
opens a coloured, striped or blurred window for tests. Verified in the
cloud container in Xvfb on llvmpipe; not yet on a real GPU.

Tests (`just comp-test`, de/comp/tests/render.rs): a headless sol-comp
(Mesa surfaceless EGL, software rendering, no display needed) runs real
`sol-testclient` windows and checks pixels: the image is upright, corners
are cut and edges are not, shadows fall below windows and follow focus,
blur smooths what is behind a window that asks for it (whole surface or
a region) and nothing else, blurred backdrops are reused while idle and
redone when what is behind moves, and idle frames have no damage.
Screenshots land in `target/tmp/sol-comp-render/`. Checked to fail when
the Stage 2 shadow-damage bug is put back, or when blur is switched off.

Not yet: server-side titlebars (Phase 5, sol-wm), every protocol beyond
xdg-shell and blur, layer-shell (the dock and menu bar need it), the DRM
backend, measured frame times. Rotated
outputs are not handled by the corner clip yet.

## Phase 3 (previous): demo works

**Milestone demo:**

```sh
nix develop
just bootstrap && just image   # as in Phase 2 (cached after the first run)
just update-demo               # build/demo/update.img: Solace 0.1.1 as an update
just qemu-test-update          # the whole demo, automated (5 boots, ~10 min without KVM)
```

By hand: `just qemu --update build/demo/update.img`, log in (root /
solace), then

```sh
mkdir -p /Volumes/Update && mount -t vfat -o ro /dev/vdb /Volumes/Update
spk --repo /Volumes/Update/good upgrade   # new system 2; the running one is untouched
spk systems
solctl reboot                             # boots system 2 (Solace 0.1.1)
solctl reboot                             # pick "Solace 0.1" in the boot menu: system 1
spk rollback                              # or make the previous system the default
```

Verified in the cloud container (2026-10-19), `just qemu-test-update`:

```
✓ A: tampered update refused; no snapshot left, boot menu unchanged
✓ A: power cut after the new system was written, before it was sealed
✓ B: system 1 still boots after the power cut (snapshots on disk: 1 2)
✓ B: update applied as system 2 (half-made snapshot cleaned up); next boot: 2
✓ C: rebooted into system 2 (Solace 0.1.1)
✓ D: picked the previous system in the boot menu: system 1 (Solace 0.1)
✓ D: spk rollback made system 1 the default
✓ E: system 1 boots by default after the rollback
qemu-test-update: PASS
```

The boot menu with both systems: `docs/design/img/boot-menu-two-systems.png`.
`just qemu-test` (Phase 2) still passes and now also checks `/boot`,
`/run/sol/btrfs` and `spk systems`.

Phase 3 stages:

| Stage | What | State |
|---|---|---|
| 1 | Design: docs/formats/system.md | done |
| 2 | sol-init mounts the btrfs top level and the ESP; bootgen passes `sol.esp=` | done, tests |
| 3 | libspk `btrfs` (ioctls) + `system` (transact, rollback, prune) | done, tests + QEMU |
| 4 | spk on Solace: live installs as transactions, `spk systems`, `spk rollback`; spk package | done |
| 5 | Update disk, `just qemu-test-update` with failure injection | done, passes |

## Phase 2 (previous): demo works; boot time to be measured on KVM


**Milestone demo:**

```sh
nix develop
just bootstrap     # cross toolchain -> temporary tools -> final system (hours once, then cached)
just image         # build/image/solace.img (files only; no real disks touched)
just qemu          # our themed GRUB menu -> text login on screen and serial (root / solace)
just qemu-test     # headless: boot, log in, solctl, os-release, root layout, sealed /usr, poweroff
```

Verified in the cloud container (2026-10-19): the full bootstrap builds
all 51 packages from source (final gcc 16.2.0 in 49 min); a rerun with no
changes rebuilds nothing (72 cached, 3 min, mostly signature checks), and
an unrelated docs change rebuilds nothing either. `just qemu-test` passes:

```
✓ sol-init: boot complete 5860 ms after the kernel started
✓ text login prompt        ("Solace 0.1 (Koi)")
✓ logged in as root
✓ solctl status / boot-time / logs
✓ /etc/os-release (from the factory /etc)
✓ Solace root layout (Applications, Users, System, Volumes)
✓ /usr is the sealed read-only snapshot
✓ clean poweroff
```

Screenshots of the real image: `docs/design/img/boot-menu-qemu.png` (our
GRUB build drawing our theme, no upstream branding) and
`docs/design/img/boot-login-tty1.png` (login on the framebuffer console).

**Not yet verified: "under 5 s after GRUB".** The container has no KVM:
the 5.9 s above is pure software emulation (TCG) on otherwise idle CPUs;
earlier runs while gcc compiled took 7.5-8.4 s. sol-init itself takes
under 1 s of it. Run `just qemu-test` on the
NixOS host (KVM is used automatically) to measure it; the result line
says whether it is under the target.

| Stage | What | State |
|---|---|---|
| 1 | Bootstrap plan + spk toolchain modes (host/cross/native/rust) | done |
| 2 | Cross, temporary and chroot-stage recipes (`bootstrap/recipes`) | done |
| 3 | Final system recipes (glibc ... gcc, kernel, GRUB, OpenSSL, pkgconf) | done |
| 4 | sol-init (PID 1, units, supervision, sol-log, solctl) | done, 39 tests |
| 5 | sol-bootgen (grub.cfg, ESP kernels) + GRUB theme | done, 15 tests |
| 6 | Packages for our own tools (sol-init, sol-bootgen, solace-base, theme) | done |
| 7 | Image builder, `just qemu`, `just qemu-test` | done |
| 8 | Boot in QEMU, measure GRUB-to-login | boots and passes; KVM timing pending |

What the first boots found (each fixed, with a test): `noatime` passed to
btrfs as an option (EINVAL on the root mount); one invalid unit name
halting boot (now skipped and logged); services sharing PID 1's session
(login could not read a password); rustix's `waitpid(None)` reaping only
our own process group (services and orphans left as zombies); spk
silently dropping a symlink from local sources (no /etc/os-release); the
boot menu dated 1980 (the dev shell's SOURCE_DATE_EPOCH). The build found:
a gawk 5.4.1 bug that miscompiled GCC's option tables (upstream patch,
everything after it rebuilt), and three spk stamp/sysroot flaws that made
every rerun rebuild everything.

Phase 1's demo still works: `just demo`.

## Done

### Phase 0
- Names (Solace / sol / spk / ace / Koi), CLAUDE.md, repo layout, flake
  devShell, justfile, solcheck, docs.

### Phase 1
- Specs: `docs/formats/recipe.md`, `package.md`, `repo.md`.
- `pkg/libspk` (library), `pkg/spk` (CLI). Modules:
  - `version`: rpm-style version compare with `~` pre-releases; dep specs.
  - `manifest`: payload paths must be relative, normalised, under `usr/`;
    symlinks may not escape the root.
  - `sign`: ed25519 with domain-separated messages; trusted keyring per
    root; secret keys 0600 and refused if readable by others.
  - `package`: reproducible `.solpkg` (tar.zst, manifest first); verify on
    a private copy, signature before decompression, index match, then
    extraction that accepts only exact manifest matches.
  - `repo` + `fetch`: signed index verified before parsing; https-only
    (redirects too) or local; size-limited, hashed downloads;
    content-addressed source cache, re-verified on every use.
  - `db`: SQLite at `usr/lib/spk/db.sqlite` (snapshots with `@system`).
  - `solve`: resolvo with versioned deps, provides, conflicts, replaces;
    clear "required by" errors on removal.
  - `tx`: locked, staged, conflict-checked transactions with a
    backup/rename journal; any failure (including the DB commit) restores
    the root exactly. Failure injection test fails every step in turn.
  - `build`: bubblewrap sandbox (all namespaces, no network, no caps,
    unprivileged; drops from root via setpriv), sysroot built through the
    verified install path, output inspected without following symlinks,
    splits, signing, repo publish, build stamps.
- CLI: install, remove, upgrade, search, info, files, owns, list, history,
  verify, build, repo-add, key gen/trust. Refuses to modify `/` on a host
  that is not Solace.
- 10 recipes with justifications: zlib, bzip2, xz, lz4, zstd, expat,
  libffi, pcre2, libxcrypt, libarchive.
- `tools/sandbox-probe`: hostile recipe; all 10 escape attempts fail.

### Brand (parallel track, allowed by CLAUDE.md section 7)
- Logo: `assets/brand/solace-icon.svg` (full colour, squircle plate) and
  `solace-mark.svg` (single colour, pixel-sharp at 16 px), and
  `solace-icon-glass.svg` (transparent clear-glass variant), generated by
  `just logo`. See HIG.md "Brand".

### Phase 2
- `bootstrap/plan.toml` + `spk bootstrap`: LFS-style stages driven by
  spk recipes. Cross toolchain for `x86_64-solace-linux-gnu`, temporary
  tools, chroot stage, then every final package rebuilt natively against
  our own glibc. Temporary packages are `~bootstrap` versions, replaced
  by the finals. Nothing from Nix ends up in a final package.
- spk: per-recipe toolchain modes, local (in-repo) sources, cargo
  vendoring for offline Rust builds, build stamps over the dependency
  closure, per-phase subshells.
- `recipes/`: final system (linux-headers, glibc, bash, coreutils,
  util-linux, shadow, pkgconf, openssl, gcc, binutils, GRUB 2.14 with
  branding patched out, linux 7.2 with `boot/kernel/solace.config`, ...),
  each justified in DECISIONS.md. Every source matches nixpkgs' hash, an
  upstream signature or checksum, or another distribution's pin, except
  bc, shadow, lz4 and libarchive (trust on first use, see DECISIONS.md).
- spk: base packages of a stage are pinned to its dependency repos;
  build stamps follow the versions actually installed; a split build dep
  swaps in its own parent; local sources refuse symlinks and can list
  `include` paths.
- `init/`: sol-init (PID 1: mounts, root assembly from btrfs subvolumes,
  switch_root, /etc seeding, sysctl, random seed, clean shutdown; service
  manager with TOML units, dependency graph, restarts, cgroups v2,
  no_new_privs; sol-log; control socket) and `solctl` (status, start,
  stop, restart, enable, disable, logs, boot-time, reboot, poweroff).
- `boot/bootgen`: sol-bootgen (atomic grub.cfg, ESP space check, pruning,
  menu styles, other-OS entries, consoles, validated inputs) and
  sol-mkinitramfs.
- `boot/grub-theme`: our gfxmenu theme (blurred wallpaper, glass logo,
  Inter); `just grub-preview` screenshots it in QEMU.
- `image/`: unprivileged image builder (btrfs `@snapshots/1` read-only,
  `@etc`, `@var`, `@users`; FAT32 ESP; GPT), QEMU runner, smoke test.

### Phase 3
- `docs/formats/system.md`: snapshots, metadata, the transaction steps,
  rollback, boot settings.
- sol-init: `/run/sol/btrfs` (top level, root-only), `/boot` (ESP by FAT
  volume id), `/run/sol/booted`; sol-bootgen writes `sol.esp=`.
- libspk: `btrfs` (snapshot, seal, delete; ABI pinned by tests),
  `system::Live` (transact, rollback, overview), read-only database
  opens, roots with an explicit keyring; `spk build` accepts deps that
  are only in dependency repos.
- spk: live install/remove/upgrade are system transactions; `spk systems`,
  `spk rollback [ID]`; packaged (`recipes/spk`) and in the image.
- image: system 1 carries its initramfs and metadata; `/etc/sol/boot.toml`.
- Tests: `just qemu-test-update` (tampered update, power cut before the
  seal, update, boot menu pick, rollback).

## Not done / limitations (honest list)

- **Boot time unmeasured on real virtualization** (Phase 2, see above).
- **Config updates do not reach /etc.** /etc is seeded from
  `/usr/share/factory/etc` on first boot only; the 3-way merge of
  changed factory files (CLAUDE.md section 4) is not implemented.
- Package triggers (`[triggers]` in recipes) are refused; the only
  trigger is the built-in initramfs rebuild.
- The boot menu keeps the newest `keep` systems plus the default. After
  a rollback followed by several updates, the running system can drop out
  of the menu (its snapshot is never deleted while it runs).
- Only local repositories work on Solace itself: the image has no CA
  certificates for HTTPS yet.
- `sol-pkgd` (spk over D-Bus for Settings and the installer) is not
  started; it arrives when Settings needs it.
- Dev images log in as root with the password `solace`
  (`SOL_ROOT_PASSWORD`); real users come with the installer (Phase 12).
- shadow is built `--disable-logind` (no libsystemd) until elogind
  arrives; util-linux has no `wall`, and `mount`/`umount` are not setuid
  (drives will be mounted through udisks).
- Packages do not declare their glibc dependency; the image lists glibc
  explicitly, and `image/build.sh` fails if any binary needs a library
  the image lacks. Automatic library dependencies are a later spk feature.
- sol-init and sol-bootgen rebuild when anything in the Cargo workspace
  changes (`include` limits them to the workspace, not to their crate).
- The GRUB theme package uses the host toolchain (`grub-mkfont` from the
  workbench; ours is built without FreeType). Its output is data from our
  sources.
- No KVM in the cloud container: QEMU there is software-emulated, so boot
  times measured there are not representative. Measure on the NixOS host.
- PAM is deferred (shadow is built `--without-libpam`); sched_ext is
  deferred until the toolchain has pahole. See DECISIONS.md.
- Rust itself is not bootstrapped: our Rust tools are compiled by the
  host's rustc and linked by our gcc against our glibc (DECISIONS.md).
- The initramfs lives on the ESP only; Phase 3 transactions regenerate it.

- Phase 1's `build/repo` packages (`just demo`) are built with the host
  toolchain and are not bootable; the system stage rebuilds them natively.
- On a scratch root (not a running Solace), a transaction that is
  *killed* mid-apply leaves `usr/lib/spk/staging/` and blocks the next
  run until removed by hand. On Solace, system transactions are crash
  safe (tested).
- No cgroup resource limits or ccache in builds yet.
- Recipe test suites (`make check`) are not run.
- `spk` has no autoremove of orphaned dependencies.
- Live HTTPS repository fetching is implemented, but only local repos
  have been exercised end to end (source downloads over HTTPS are
  exercised by every clean build).

## Broken / not verified

- Verified only in the cloud dev container, not yet on the human's NixOS
  host (where KVM makes boots fast and the 5 s target measurable).
- `spk bootstrap` must not run twice at once against the same repos (the
  index has no lock); the image builder only reads them.

## Open questions for the human

1. **Desktop name:** recorded as DE = Koi, release label "SolOS 1: Koi",
   OS = Solace. Is "SolOS" meant to replace "Solace"?
2. GPL-3.0-or-later was chosen as the default; say if you want
   GPL-3.0-only instead.

## Next

1. On the NixOS host: measure the Phase 2 boot time with KVM
   (`just qemu-test`), and try the Phase 3 demo with a window
   (`just qemu --update build/demo/update.img`).
2. Phase 4: compositor core (sol-comp, Smithay): nested in the host
   desktop first, then DRM on the QEMU image. It needs the graphics base
   (Mesa, libdrm, libinput, Wayland, ...) as recipes first.
