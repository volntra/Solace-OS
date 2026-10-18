PROJECT BRIEF: SOLACE - A MACOS-STYLE OPERATING SYSTEM ON A MINIMAL LINUX BASE
For Claude Code. Read this whole file at the start of every session, then
read docs/STATUS.md to see where we are. Work one phase at a time, in order.
When the human says "start Phase N" or "continue", resume from STATUS.md.


== 0. NAMES (fixed in Phase 0, 2026-10-18) ==

The names are settled; do not ask about them again.

  OS    Solace    Operating system name (branding, boot screen, About)
  PFX   sol       prefix for binaries, D-Bus names (org.sol.*),
                  config paths
  PKG   spk       Package manager command
  SH    ace       Command shell command
  DE    Koi       Umbrella name for the desktop (compositor + WM + shell
                  + apps). Release label for version 1: "SolOS 1: Koi"
                  (see docs/DECISIONS.md, 2026-10-18).

App names are our own, never Apple's: "Files" not "Finder", "Search" not
"Spotlight", "Overview" not "Mission Control".


== 1. MISSION ==

Solace is its own operating system that uses the Linux kernel as its deep
base, the way Android and ChromeOS do. It is NOT a Linux distribution in
spirit: users never see a "Linux desktop". Everything above the kernel
and a minimal set of low-level libraries is ours:

  - Our own init and service manager
  - Our own sealed, read-only system image with a macOS-style layout
  - Our own package manager, which builds the system from source recipes
    and also installs app bundles
  - Our own boot menu generator and GRUB theme
  - Our own Wayland compositor, and a separate window manager process
  - Our own desktop shell: menu bar, dock, search, notifications, control
    center
  - ADAPTIVE ICONS that recolor to the wallpaper, the accent color, and
    light/dark mode (centerpiece feature)
  - Our own login screen and lock screen
  - Our own Settings, Files, and Terminal apps
  - Our own command shell
  - A MACOS-STYLE INSTALLER with deep install-time customization
    (centerpiece feature)

The feel is macOS: calm, polished, consistent, with translucent
materials, a global menu bar, and spring-based motion, but with original
artwork and our own identity.

Development host: the human's NixOS machine. Target hardware: QEMU first,
then the human's PC (AMD Ryzen 7 7800X3D, 32 GB RAM, Samsung 990 PRO 4TB
NVMe, NVIDIA RTX 5070 Ti, 2560x1440 @ 165 Hz). NVIDIA is a first-class
target.


== 2. GROUND RULES FOR CLAUDE ==

1.  Phases are sequential. Do not start a phase until the previous
    phase's milestone demo works.
2.  After every session, update docs/STATUS.md: what's done, what's in
    progress, what's broken, and the exact command to run the current demo.
3.  Log every non-trivial decision in docs/DECISIONS.md (date, decision,
    alternatives, why). The stack in section 3 is locked. Change it only
    if something is truly unworkable, and record why.
4.  No silent stubs. Never present a stub, mock, or todo!() as finished.
    Mark incomplete work in STATUS.md and in your reply.
5.  Every milestone has a runnable demo as a `just` recipe.
6.  Commit at every working checkpoint, in small commits with clear
    messages.
7.  Security-critical code gets tests and extra care: package signatures
    and transactions, sandboxed builds (never run build scripts as root),
    PID 1, boot configuration generation, the login and lock screens,
    anything privileged over D-Bus, and the installer's disk-writing code.
8.  Never write to a real disk. All disk work happens on QEMU images
    unless the human explicitly approves a specific device in the current
    session.
9.  No Apple assets: no SF fonts, SF Symbols, Apple icons, wallpapers,
    sounds, or trademarked names. Recreate the feel with original artwork
    and permissively licensed fonts and symbols.
10. "Our own" means our own product and architecture. Use proven
    libraries for protocols, parsing, and crypto rather than reinventing
    them.
11. The user should never need to know Linux is underneath. No upstream
    branding (including GRUB's), no stray terminal windows, no unthemed
    dialogs.
12. Ask the human only about real decisions: names, taste calls, and
    anything touching real disks. Otherwise pick the sensible option, log
    it, and continue.


== 3. LOCKED TECHNOLOGY STACK ==

THE DEEP BASE (upstream, minimal, kept small on purpose):
Kernel           Mainline Linux, our own trimmed config, built by our
                 recipes. sched_ext enabled.
C library        glibc (NVIDIA userspace and Steam require it).
Toolchain        GCC + binutils; LLVM where Mesa needs it.
Bootloader       GRUB 2 (x86_64-efi) with our own gfxmenu theme. grub.cfg
                 is written by our generator (sol-bootgen), never by
                 grub-mkconfig.
Device mgmt      udev built standalone from the systemd source tree (as
                 Gentoo's systemd-utils does). NOT the rest of systemd.
Sessions/seats   elogind (standalone logind API: seats, sessions,
                 suspend, polkit integration).
IPC bus          dbus-daemon (dbus-broker's launcher needs systemd).
Auth             Linux-PAM, polkit.
Graphics         Mesa, NVIDIA open kernel modules + NVIDIA userspace (GBM),
                 libdrm, libinput.
Audio            PipeWire + WirePlumber.
Network / BT     NetworkManager, BlueZ (backends only; all UI is ours).
Filesystem       btrfs (zstd), optionally on LUKS2.
X11 apps         xwayland-satellite.
Third-party apps Flatpak supported out of the box (Steam, browsers, VR).

OURS:
Init + services  sol-init, Rust: PID 1 + service manager.
Package manager  spk, Rust.
Boot config      sol-bootgen, Rust: generates grub.cfg + manages
                 kernels on the ESP.
Compositor core  sol-comp, Rust + Smithay.
Window manager   sol-wm, Rust, separate process speaking a private
                 Wayland protocol with the core.
Shell UI + apps  Qt 6 / QML + C++, with a shared design-system QML module.
Settings backend sol-settingsd, Rust, D-Bus, TOML storage.
Command shell    ace, Rust.
Login            Our own greeter on greetd.
Icons            sol-icons, Rust engine with a C API and Qt image
                 provider.
Installer        Qt app.
Fonts/symbols    Inter (UI), JetBrains Mono (mono), Phosphor or Lucide
                 symbols, plus our own icons.
Build            just + Cargo + CMake/Ninja; dev environment from a Nix
                 flake devShell on the NixOS host.


== 4. SYSTEM LAYOUT ==

The system is sealed and read-only during normal use. Users see a
macOS-style hierarchy; the Linux tree is real but hidden.

User-visible (shown in Files):
  /Applications        app bundles (one directory each)
  /Users/<name>        home directories (/home -> /Users)
  /Volumes             mounted drives
  /System              read-only OS (a view of /usr; read-only label)

Underneath (hidden by Files unless "Show system files" is on):
  /usr                 the whole OS, merged-usr, read-only btrfs subvolume
  /bin /sbin /lib      symlinks into /usr
  /etc                 writable, seeded from /usr/share/factory/etc on
                       first boot; spk 3-way merges config updates
  /var                 writable
  /boot                ESP, FAT32, 2 GiB: GRUB EFI binary, our GRUB theme,
                       grub.cfg, and one kernel + initramfs per bootable
                       system snapshot in /boot/sol/<snapshot-id>/

btrfs subvolumes: @system (/usr, read-only), @etc, @var, @users,
@snapshots. Updates create a new @system snapshot and switch to it
atomically. The previous system stays bootable from the GRUB menu.


== 5. REPOSITORY LAYOUT ==

CLAUDE.md
justfile
flake.nix
docs/
  STATUS.md
  DECISIONS.md
  design/HIG.md             spacing, radii, colors, materials, motion
  protocols/                private Wayland and D-Bus protocol specs
  formats/                  recipe, package, bundle, unit-file formats
recipes/                    build recipes for every system package
bootstrap/                  cross toolchain + stage builds driven by spk
image/                      system image, live installer image, QEMU runner
init/                       sol-init
boot/
  bootgen/                  sol-bootgen
  grub-theme/               our GRUB gfxmenu theme
pkg/                        spk (CLI, daemon, library)
sh/                         ace + sol-utils
de/
  comp/                     compositor core
  wm/                       window manager
  protocols/
  settingsd/
  design/                   QML design system
  shell/                    menu bar, dock, search, notifications,
                            control center
  icons/                    adaptive icon engine + icon set
  greeter/                  login + lock screen
  portal/                   xdg-desktop-portal backend
apps/
  settings/
  files/
  terminal/
  installer/
assets/                     original icons, wallpapers, sounds, boot art
tools/                      repo tooling (solcheck); see DECISIONS.md


== 6. COMPONENT SPECIFICATIONS ==

--- 6.1 Init and service manager: sol-init ---
- PID 1 in Rust: mounts, early setup (hostname, clock, random seed,
  sysctl, kernel modules), then starts services.
- Service units in TOML (docs/formats/unit.md): command, user, deps,
  restart policy, readiness (notify fd or socket), resource limits
  (cgroups v2), sandboxing (namespaces, no_new_privs, read-only paths).
- Parallel startup from the dependency graph. Supervises and restarts
  crashed services.
- Structured log store (sol-log), viewable in a Logs pane in
  Settings and via `solctl logs`.
- `solctl`: start, stop, restart, status, enable, disable, logs,
  reboot, poweroff.
- Clean shutdown: stop services in reverse order, sync, unmount, then
  reboot or power off.
- Target: GRUB handoff to login screen in under 5 seconds on the 990 PRO.
  Measure every boot and log it.

--- 6.2 Package manager: spk ---
Two kinds of packages:
  SYSTEM packages   built from recipes/, installed into a new @system
                    snapshot. Never installed one-by-one onto the live
                    system.
  APP BUNDLES       self-contained directories in /Applications
                    (<Name>.solapp/) with a manifest, icon in our
                    adaptive format, and the binary + private libraries.
                    Install/remove is just adding or removing a
                    directory, plus registering the app.

Recipes (recipes/<name>/recipe.toml): name, version, sources + sha256,
build/runtime deps, split packages, build steps, triggers.

Builds run unprivileged in a sandbox (bubblewrap/user namespaces), with no
network after source fetch. Output: .solpkg = tar.zst + manifest
(files + hashes + deps) + detached ed25519 signature.

Repos: a signed index (ed25519) + packages over HTTPS. Unsigned or
mismatched packages are refused, and no flag skips that.

Dependency solver: the resolvo crate (SAT-based), with versioned deps,
provides, conflicts, and replaces.

System update transaction:
  resolve -> download -> verify -> snapshot current @system as a
  writable copy -> apply changes -> run triggers inside it (initramfs
  rebuild included) -> seal it read-only -> run sol-bootgen to copy
  its kernel + initramfs to the ESP and add it as the new default GRUB
  entry -> takes effect on reboot (live apply only for app bundles).
  Failure at any step discards the new snapshot and leaves grub.cfg
  untouched; the running system is never touched.

Rollback: every previous system snapshot (up to the configured count) is
a GRUB menu entry, and `spk rollback` / Settings > Updates can make
one the default.

CLI: install, remove, upgrade, search, info, files, owns, history,
rollback, build, repo-add. Clean, colored output, one progress bar per
stage.

Daemon sol-pkgd on the system bus, polkit-guarded, so Settings and
the installer can drive it. `--root <path>` works for everything.

--- 6.3 Base bootstrap ---
Automate what Linux From Scratch does by hand, driven entirely by
spk recipes:
  1. Build a cross toolchain in an isolated container on the NixOS host.
  2. Build stage-1 temporary tools, then chroot and build every final
     system package from recipes/.
  3. Every file in @system must come from a .solpkg. No loose
     `make install`s.
  4. ccache + parallel builds; rebuilding only changed recipes must be
     cheap.
  5. image/ assembles the btrfs subvolume layout, GRUB on the ESP,
     sol-bootgen's grub.cfg, and the initramfs (sol-init can run
     as its own initramfs init for LUKS unlock), and boots it in QEMU
     (OVMF, virtio-gpu).

Keep the base minimal. Every upstream package added to recipes/ needs a
one-line justification in docs/DECISIONS.md (enforced by `just check`;
see the "Package justifications" section there).

--- 6.4 Boot: GRUB + sol-bootgen ---
- GRUB 2 (x86_64-efi) is installed to the ESP. It never needs to read
  btrfs or unlock LUKS: kernels and initramfs images live on the ESP
  (GRUB's LUKS2 support lacks Argon2, our default).
- sol-bootgen (Rust) owns grub.cfg:
  - one entry per bootable @system snapshot (newest first, labeled with
    version + date), copying that snapshot's kernel + initramfs to
    /boot/sol/<snapshot-id>/ and passing rootflags=subvol=<id>
  - removes kernels for snapshots that have been pruned
  - adds entries for other OSes found by os-prober (if the user kept
    them)
  - writes grub.cfg atomically (write temp file, fsync, rename)
  - sets the default entry, timeout, and menu style from settingsd
    (always show / hidden unless Shift or Esc is held / never)
  - checks free space on the ESP before copying and refuses cleanly if
    it would fill up
- Our own GRUB gfxmenu theme: blurred wallpaper background, centered
  entry list in our typography, our logo, no GRUB branding anywhere
  (including the "GNU GRUB" header and help text).
- Verbose vs graphical boot is a kernel command-line choice that
  bootgen writes.
- spk calls bootgen as the last step of every system transaction
  and rollback.
- Tests: generate configs for 1, 5, and 20 snapshots plus other-OS
  entries, and boot each entry in QEMU.

--- 6.5 Compositor core: sol-comp ---
- Backends: winit (nested, for development inside the human's current
  desktop) and DRM/KMS + libinput (real sessions, via elogind/libseat).
- Renderer: Smithay GLES with custom shaders for rounded corners, soft
  shadows, and dual-Kawase background blur, with damage tracking.
- Protocols at minimum: xdg-shell, xdg-decoration, layer-shell,
  xdg-activation, wlr foreign-toplevel (with set_rectangle for
  minimize-to-dock), screencopy, output-management, presentation-time,
  fractional-scale, viewporter, linux-drm-syncobj (explicit sync,
  required for NVIDIA), tearing-control, VRR, session-lock, idle-notify,
  idle-inhibit, pointer-constraints, relative-pointer, text-input and
  input-method, data-control, the KDE appmenu protocol (global menu),
  and a blur protocol for clients that request it.
- Animation engine: critically damped springs, not fixed durations.
- Stable 165 Hz on the dev machine with three windows open plus the dock
  blurring.
- If the WM process crashes, the core keeps every window on screen,
  restarts the WM, and re-sends state.

--- 6.6 Window manager: sol-wm ---
A separate process. The core reports windows, outputs, and input; the WM
returns geometry, stacking, focus, decorations, and animation targets
(spec in docs/protocols/wm.md).
- Floating by default with smart placement.
- Server-side titlebars with traffic-light buttons top-left (close /
  minimize / zoom), glyphs on hover, dimmed when inactive. Double-click on
  the titlebar zooms.
- Edge and corner tiling, Spaces (virtual desktops), fullscreen in its
  own Space.
- Minimize into the dock with a scale/genie animation toward the icon.
- Overview: all windows tiled, with a Spaces strip on top.
- App switcher (Super+Tab) grouped by app; Super+` cycles windows within
  an app.
- Shadows tied to focus.

--- 6.7 Design system: de/design/ ---
A QML module every shell component and app imports:
- Light and dark palettes plus the accent, with every contrast pair
  checked against WCAG AA.
- Materials: window, sidebar, popover, menu, HUD, sheet. Each is a blur +
  tint + subtle noise.
- Typography scale (Inter), a 4px spacing grid, corner radii, and spring
  presets.
- Controls: buttons, toggles, checkboxes, segmented controls, sliders,
  steppers, text fields, search fields, popovers, sheets, alerts, menus,
  sidebars, toolbars with unified titlebars, tables, outline views, and
  column views.
- Documented in docs/design/HIG.md with screenshots.
- Ships matching Qt and GTK themes so Flatpak and third-party apps don't
  look out of place. The GRUB theme uses the same palette and type.

--- 6.8 Desktop shell UI: sol-shell ---
- Menu bar (top, translucent): Solace menu (About, Settings, Sleep,
  Restart, Shut Down, Lock, Log Out), bold active app name, global menu
  (KDE appmenu + com.canonical.dbusmenu, with a fallback for apps that
  don't export one), then tray (StatusNotifierItem), control center,
  search, and clock on the right.
- Dock: pinned apps, running indicators, a recent-apps section, and
  Downloads/Trash stacks. Optional magnification and autohide; position
  bottom/left/right; drag to reorder; right-click menus; launch and
  attention bounce; badges and progress bars via
  com.canonical.Unity.LauncherEntry.
- Search (Super+Space): apps, settings panes, files (our own indexer in
  Rust, SQLite FTS5 + fanotify/inotify), calculator, unit conversion.
  Keyboard-first.
- Notifications: an org.freedesktop.Notifications server, banners
  top-right, a history panel, Do Not Disturb.
- Control center: Wi-Fi, Bluetooth, sound output and volume, brightness,
  night shift, DND, light/dark toggle, media controls (MPRIS).
- Volume/brightness OSDs, a screenshot and recording tool, wallpaper with
  crossfades plus light/dark dynamic pairs.

--- 6.9 Adaptive icon system (centerpiece) ---
Icon format: a directory <app-id>.solicon/ containing
  icon.toml           metadata, default colors, which layers are tintable
  background.svg      shape fill (gradients allowed)
  foreground.svg      glyph, with fills tagged by role: primary,
                      secondary, highlight
  dark-*.svg          optional dark-mode layers
Every icon is masked to our own continuous-curvature squircle
(superellipse), defined once in de/design.

Render modes, chosen in Settings and in the installer:
  1. Default            as designed
  2. Dark               designer's dark layers if present; otherwise an
                        auto near-black background with foreground hues
                        kept and lightness clamped for contrast
  3. Tinted             the foreground becomes a luminance mask colored
                        with the accent; the background becomes a dark
                        accent-tinted gradient
  4. Wallpaper-adaptive like Tinted, but the accent is extracted from the
                        current wallpaper (HCT, Material Color Utilities
                        algorithm, Apache-2.0). Changing the wallpaper
                        crossfades every icon to the new palette.
  5. Clear              frosted-glass background, white foreground

Legacy icons (freedesktop PNG/SVG from Flatpak and third-party apps) are
placed on a squircle plate colored by their dominant color. In modes 3-5
they go through a luminance-to-alpha pass and get tinted like native
icons, so the dock never looks mismatched.

Engine: Rust (resvg for SVG), with a C API + Qt image provider
(image://solicon/...). GPU rendering where possible; disk cache in
~/.cache/sol/icons/ keyed by (icon hash, mode, accent, size, scale).
It listens to settingsd. Target: re-tinting the full dock in under 16 ms
with a warm cache.

Icon set: original icons for every built-in app, system folders, volumes,
and file types, plus overrides for the ~40 most common Flatpak apps.

--- 6.10 Settings daemon: sol-settingsd ---
A typed schema per domain (appearance, dock, wm, input, displays, boot,
...) stored as TOML. D-Bus get/set/watch with validation and change
signals. Every component reads settings through it; nothing reads the
files directly. Changes to the boot domain trigger sol-bootgen.

--- 6.11 Login and lock screen ---
- Our greeter on greetd, running in a minimal kiosk instance of our
  compositor: blurred wallpaper, large clock, user avatars, password
  field, power buttons, keyboard-layout indicator.
- Smooth fade/zoom from login into the desktop (the greeter hands off
  its last frame).
- Lock screen via ext-session-lock-v1, sharing the greeter's look,
  unlocking through PAM. It stays locked if the shell UI or WM crashes.
- Boot is seamless: our GRUB theme (or no menu at all, if hidden) ->
  sol-init splash (our logo + progress on the framebuffer, or a LUKS
  passphrase prompt in our style) -> greeter, with no text flashes unless
  verbose boot is on. GRUB is set to keep the framebuffer mode
  (gfxpayload=keep) so there's no mode-switch flicker.

--- 6.12 Settings app ---
Sidebar + searchable content, macOS System Settings style. Panes:
- Appearance: light/dark/auto, accent (including wallpaper-derived), icon
  mode, window corner style, reduce motion, reduce transparency, text size
- Wallpaper, Dock & Menu Bar, Desktop & Spaces
- Displays: resolution, refresh rate, VRR, scale, arrangement
- Sound, Network, Bluetooth, Notifications, Keyboard, Mouse
- Users & Login, Privacy (portal permissions)
- Updates: via pkgd, with update history and system snapshot rollback
- Storage: disk usage, snapshots, compression savings, ESP space
- Startup & Boot: GRUB menu style and timeout, theme vs plain text,
  verbose boot, default system snapshot, other-OS entries
- Performance: sched_ext scheduler profile (see installer)
- Logs (from sol-log), Date & Time
- About: logo, version, hardware summary

--- 6.13 Files ---
- Sidebar: Favorites, Applications, Users, Volumes, Tags, Trash
- Icon, list, column, and gallery views, with column view first-class
- Quick-look preview on Space: images, PDF, text/code with
  highlighting, audio/video
- Tags in user.xdg.tags xattrs, Trash per the freedesktop spec
- Async copy/move with a progress panel, Get Info window, tabs, path bar,
  Open With, mounting via udisks2 into /Volumes
- Hides the Linux tree by default (section 4)
- Its components power the portal FileChooser (6.16), so every app's
  open/save dialog is ours

--- 6.14 Terminal ---
Qt app with a custom GPU-rendered QQuickItem and libvterm for emulation.
Tabs, splits, profiles, ligatures, true color, OSC 8 hyperlinks, OSC 133
semantic prompts (jump between prompts, select command output), kitty
keyboard protocol, a translucent material, and colors that follow the
accent and light/dark mode.

--- 6.15 Command shell: ace ---
Rust, interactive-first, fish-like ergonomics, not POSIX (bash remains
/bin/sh for scripts).
- Autosuggestions from history, live syntax highlighting, rich
  completions (generated from man pages, plus a completion spec format)
- Clean scripting language with real lists/maps and sane quoting
- Structured config in ~/.config/ace/config.ace
- Built-in prompt with exit code and git status, emitting OSC 133 marks
- Startup under 20 ms
- Default shell for new users

--- 6.16 Portals ---
An xdg-desktop-portal backend: FileChooser (Files components),
Screenshot, ScreenCast (PipeWire), Settings (color scheme, accent),
Notification, OpenURI, Inhibit, Background. Flatpak apps, browsers, OBS,
and Discord screen-share must work.

--- 6.17 Installer (centerpiece) ---
Live image: boots from USB (GRUB with our theme) straight into a minimal
Solace desktop running only the installer. It looks and moves like macOS
Setup: full-screen blurred wallpaper, a centered card that changes with
spring transitions, a back button, and a step indicator.

Two paths on the welcome screen:
  EXPRESS   language -> disk -> account -> install. Sensible defaults for
            everything else.
  CUSTOM    every option below. Each screen has a "Reset to defaults"
            link and a one-line explanation under each option.

Custom install options:

Language & region
  - Language, region formats, keyboard layout (with a live test field),
    time zone (clickable world map + search), 12/24-hour clock

Disk
  - Guided: erase a whole disk
  - Alongside: install into existing free space (never shrinks another
    OS's partitions in 0.1). Reuses an existing ESP if it has enough room,
    otherwise creates our own.
  - Manual: a partition editor (create, delete, resize our own
    partitions, set types), with a visual bar of the disk layout
  - Shows every disk's model, size, and existing partitions/OSes, so the
    user can't confuse drives

Storage
  - Encryption (LUKS2) on/off + passphrase with a strength meter +
    "unlock also logs me in"
  - Compression: off / zstd level presets (fast, balanced, max)
  - Separate Users subvolume quota on/off
  - Snapshot policy: system snapshots kept (count), plus optional
    automatic Users snapshots (off / daily / hourly)
  - Swap: zram size, swap file size, or off; hibernation on/off (needs a
    swap file at least the size of RAM, and the installer says so)

Boot
  - GRUB menu: always show / hidden unless Shift is held / never, plus
    timeout
  - Boot menu theme: Solace theme or plain text
  - Graphical or verbose boot
  - Make Solace the default boot entry, and keep detected entries for
    other OSes (os-prober)
  - Number of previous systems to keep bootable (shows the ESP space it
    needs)

Performance
  - CPU scheduler: kernel default or a sched_ext profile (balanced /
    gaming-responsive / throughput), each with a one-line description
  - Power profile: performance / balanced / power-saver

Graphics
  - Auto-detected GPU driver, shown to the user (NVIDIA open modules,
    Mesa). Offers to install NVIDIA support when an NVIDIA GPU is found.

Components (checkbox list with sizes and a running total)
  - Optional apps, Flatpak + Flathub on/off, Steam (Flatpak) pre-install,
    developer tools, extra wallpapers, extra fonts, language packs

Appearance (with a LIVE PREVIEW panel: a miniature desktop with menu
bar, dock, and a window that updates instantly)
  - Light / dark / auto
  - Accent color: presets, a custom color, or "from wallpaper"
  - Icon mode: all five, previewed on the mini dock
  - Wallpaper from the bundled set
  - Dock: position, size, magnification, autohide, which apps are pinned
  - Menu bar: translucent or solid
  - Window corner radius preset, animation speed / reduce motion, reduce
    transparency
  - Text size

Account
  - Full name, username (auto-suggested), password with hint, avatar
    (bundled set or a monogram with color), hostname
  - Auto-login on/off (blocked if encryption is off, with an explanation)

Command line
  - Default shell (ace or bash), terminal theme, prompt style preview

Network (optional)
  - Connect now to fetch updates during install, mirror selection

Summary
  - Every choice on one scrollable sheet, each with an Edit link that
    jumps to its screen
  - A plain sentence on what will be erased: "This will ERASE <model>
    <size> and everything on it." Express needs one confirm; Custom
    needs the user to type the disk's name.
  - "Export install profile" saves all choices to a TOML file;
    "Import profile" on the welcome screen loads one. Profiles also
    enable fully unattended installs from the live image's GRUB menu.

Install
  - Progress bar with a time estimate, current step text, and a "Show
    log" toggle
  - Cancel is allowed until the first disk write, with a clear "point of
    no return" marker
  - Uses spk --root to lay down @system, then writes /etc, the user,
    and settings, installs GRUB to the ESP (grub-install
    --target=x86_64-efi with our own bootloader ID), and runs
    sol-bootgen
  - Done screen: Restart, with a countdown

First boot
  - If the user chose "Customize later" on any appearance screen, a short
    setup assistant runs on first login with the same screens.

Every installer path (Express, Custom erase, Alongside, Manual,
Encrypted, Profile import, Unattended) has an automated QEMU test that
installs to a blank image and boots the result through GRUB.


== 7. PHASES AND MILESTONES ==

Phase 0   Scaffold: names, repo layout, flake devShell, justfile, docs.
          Demo: `nix develop` then `just check` passes.

Phase 1   spk core: recipes, sandboxed builds, signing, install into
          --root, solver, DB.
          Demo: build and install 10 recipes into a scratch root.

Phase 2   Base bootstrap + sol-init + GRUB + sol-bootgen + system
          image layout (section 4).
          Demo: `just qemu` boots through our themed GRUB menu into our
          own sealed system and reaches a text login in under 5 s after
          GRUB; solctl works.

Phase 3   System transactions + rollback.
          Demo: update the image, reboot into the new snapshot, roll back
          by picking the previous system in the GRUB menu.

Phase 4   Compositor core.
          Demo: nested in the host desktop first (corners, shadows, blur),
          then DRM on the QEMU image, then real NVIDIA hardware at 165 Hz.

Phase 5   Window manager.
          Demo: traffic lights, tiling, Spaces, Overview, springs, WM
          crash recovery.

Phase 6   settingsd + design system + shell UI.
          Demo: menu bar, dock, search, notifications, control center all
          live.

Phase 7   Adaptive icons + icon set.
          Demo: cycling all five modes and changing the wallpaper recolors
          the dock live.

Phase 8   Settings app.
          Demo: every pane in 6.12 reads and writes real settings,
          including GRUB menu options.

Phase 9   Greeter + lock + seamless boot.
          Demo: GRUB -> splash -> greeter -> desktop with no text flashes;
          the lock survives `pkill sol-shell`.

Phase 10  ace + Terminal.
          Demo: a new user's default shell is ace in our Terminal; OSC
          133 jump works.

Phase 11  Files + portals + app bundles + Flatpak.
          Demo: column view, Quick Look, our file dialog in a Flatpak
          browser, screen-share works, Steam (Flatpak) launches a game.

Phase 12  Installer + live image.
          Demo: every installer path passes its automated QEMU test.

Phase 13  Real hardware.
          Demo: the live USB boots on the human's PC; install to a disk
          the human explicitly approves; boot and use it.

Phase 14  Polish: HIG audit of every screen (GRUB theme included),
          performance profiling, accessibility (keyboard navigation
          everywhere, basic screen reader), 24-hour soak test in QEMU,
          release notes for 0.1.

ace, icon artwork, the GRUB theme, and wallpapers don't depend on the
graphics stack and can be worked on in parallel if the human asks.


== 8. TESTING ==

- Rust: cargo test + cargo clippy -D warnings in every crate. The
  package manager's transaction, signature, and rollback paths need
  failure-injection tests (corrupt download, bad signature, power loss
  mid-transaction simulated by killing QEMU). After any simulated
  failure, grub.cfg must still boot the previous system.
- sol-bootgen: config generation tests for many snapshot counts,
  other-OS entries, a nearly full ESP, and every menu style.
- sol-init: tests for dependency ordering, restart policies, and
  clean shutdown.
- Compositor/WM: headless protocol tests plus screenshot tests for
  decorations and blur.
- QML: qmltestrunner for design-system controls.
- `just qemu-test` boots the image headless through GRUB and runs a
  scripted smoke test: log in through greetd, launch every app, take
  screenshots, fail on any crash in the logs.
- Real hardware checks on the NVIDIA machine at the end of Phases 4, 6,
  9, and 13.


== 9. DEFINITION OF DONE (0.1) ==

A person can boot the live USB on a real PC, choose Express or Custom,
customize disk, storage, boot, performance, and appearance options with a
live preview, install, and reboot into Solace through our themed GRUB
menu. They log in at our login screen and use a desktop where the menu
bar, dock, adaptive icons, Settings, Files, Terminal, and ace all
work; install an app bundle, a Flatpak, and a system update; roll back to
the previous system from the GRUB menu; play a Steam game; and never see
anything that looks like a generic Linux desktop along the way.
