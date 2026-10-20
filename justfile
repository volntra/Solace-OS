# Fern task runner. Run inside `nix develop` (or with direnv).

set shell := ["bash", "-euo", "pipefail", "-c"]

inter_url := "https://github.com/rsms/inter/releases/download/v4.1/Inter-4.1.zip"
inter_sha := "9883fdd4a49d4fb66bd8177ba6625ef9a64aa45899767dde3d36aa425756b11e"

# List recipes
default:
    @just --list --unsorted

# --- Current milestone demo -------------------------------------------------

# Run the demo for the current phase (see docs/STATUS.md)
demo: spk
    tools/demo-phase1.sh

# --- spk ----------------------------------------------------------------------

# Build the spk binary (release)
spk:
    cargo build --release --locked -p spk

# Build every recipe into build/repo (sandboxed; unchanged recipes are skipped)
build-recipes: spk
    @test -f build/keys/dev.key || target/release/spk key gen dev --dir build/keys
    target/release/spk build --all --toolchain host

# Run a hostile recipe that must fail to escape the build sandbox
sandbox-probe: spk
    @test -f build/keys/dev.key || target/release/spk key gen dev --dir build/keys
    target/release/spk build --force --toolchain host --recipes-dir tools/sandbox-probe --repo-dir build/probe-repo --work build/probe-work probe
    @grep -h "ok:" build/probe-work/probe/build.log

# Build the system from source, stage by stage (bootstrap/plan.toml).
# First run takes hours; finished recipes are skipped on later runs.
bootstrap *ARGS: spk
    @test -f build/keys/dev.key || target/release/spk key gen dev --dir build/keys
    target/release/spk bootstrap {{ARGS}}

# Fill build/sources from the Nix binary cache where nixpkgs pins the same hash
seed-sources:
    tools/seed-sources-from-nix.sh build/sources

# Regenerate the logo SVGs and their PNG renders in assets/brand/
logo:
    python3 tools/brand/gen-logo.py
    resvg -w 512 assets/brand/fern-icon.svg assets/brand/fern-icon-512.png
    resvg -w 128 assets/brand/fern-mark.svg assets/brand/fern-mark-128.png
    resvg -w 512 assets/brand/fern-icon-glass.svg assets/brand/fern-icon-glass-512.png
    resvg -w 1350 build/brand/glass-preview.svg docs/design/img/glass-preview.png

# Render the wallpapers (assets/wallpapers/*.svg) to PNG under build/
wallpapers:
    python3 assets/wallpapers/gen-wallpapers.py
    mkdir -p build/wallpapers
    resvg -w 2560 assets/wallpapers/grove.svg build/wallpapers/grove.png
    resvg -w 2560 assets/wallpapers/grove-blurred.svg build/wallpapers/grove-blurred.png

# Build the GRUB theme and fonts into build/grub-theme (Inter is fetched and hash-checked)
grub-theme:
    @mkdir -p build/sources
    @test -f build/sources/{{inter_sha}} || (curl -sSLf -o build/sources/{{inter_sha}}.part {{inter_url}} && echo "{{inter_sha}}  build/sources/{{inter_sha}}.part" | sha256sum -c --quiet && mv build/sources/{{inter_sha}}.part build/sources/{{inter_sha}})
    cp build/sources/{{inter_sha}} build/Inter-4.1.zip
    boot/grub-theme/build.sh build/grub-theme/theme build/grub-theme/fonts build/Inter-4.1.zip assets

# Boot the real GRUB with our theme in QEMU and screenshot the menu
grub-preview: grub-theme
    tools/grub-preview.sh

# --- System image (Phase 2) -------------------------------------------------

# Assemble build/image/fern.img from the system repo (no real disks touched)
image *ARGS: spk
    image/build.sh {{ARGS}}

# Boot the image in QEMU (UEFI, our GRUB menu, console mirrored here)
qemu *ARGS:
    image/qemu.sh {{ARGS}}

# Boot headless, log in over serial, check fernctl and the sealed /usr
qemu-test:
    image/qemu-test.py

# --- Compositor (Phase 4) ---------------------------------------------------

# Run fern-comp nested in your desktop, optionally with a client:
# `just comp weston-terminal`
comp *CLIENT:
    cargo run -p fern-comp -- --nested {{ if CLIENT != "" { "-- " + CLIENT } else { "" } }}

# fern-comp on your real display, from a text console (switch with
# Ctrl+Alt+F3, log in, cd here, `nix develop`, then this): three animated
# test windows and a dock-sized glass panel that re-blurs them every frame
# (the CLAUDE.md 6.5 load), frame timing every 5 s in build/comp-drm.log.
# Ctrl+Alt+Backspace quits. Uses the host's GPU driver (NixOS:
# /run/opengl-driver, NVIDIA's EGL and GBM among them).
comp-drm SIZE="1600x1000":
    cargo build --release -p fern-comp
    @mkdir -p build
    FERN_COMP_STATS=1 \
      LD_LIBRARY_PATH="/run/opengl-driver/lib:${LD_LIBRARY_PATH:-}" \
      GBM_BACKENDS_PATH=/run/opengl-driver/lib/gbm \
      __EGL_VENDOR_LIBRARY_DIRS=/run/opengl-driver/share/glvnd/egl_vendor.d \
      target/release/fern-comp --drm -- sh -c ' \
        c=target/release/fern-testclient; \
        $c --animate --size {{SIZE}} --title one & sleep 0.5; \
        $c --animate --size 1100x700 --title two & sleep 0.5; \
        $c --animate --size 900x600 --title three & sleep 0.5; \
        $c --color ffffff30 --blur --size 1200x110 --title dock & \
        wait' \
      2> build/comp-drm.log || true
    @grep -E "display ready|session|fps|WARN|ERROR" build/comp-drm.log | tail -20 || true

# Compositor pixel tests: headless fern-comp, real clients, screenshots
# (kept in target/tmp/fern-comp-render/). Also part of `just check`.
comp-test *SCENARIO:
    cargo test -p fern-comp --test render -- {{SCENARIO}}

# Phase 4 renderer demo: a striped window behind a frosted-glass one
# (blur, rounded corners, shadows, wallpaper)
comp-demo:
    cargo build -p fern-comp
    target/debug/fern-comp --nested -- sh -c 'target/debug/fern-testclient --stripes --size 640x400 & sleep 1; exec target/debug/fern-testclient --size 900x200 --color f4f1fa40 --blur --title Glass'

# Phase 4 demo on the image: fern-comp on the virtio GPU (KMS, libinput,
# libseat), test windows, blur, VT switching; screenshots in build/image/
qemu-test-comp:
    image/qemu-test-comp.py

# --- System updates (Phase 3) -----------------------------------------------

# Build the demo update (fern-base 0.1.1) and the update disk carrying it
update-demo: spk
    image/update-demo.sh

# Phase 3 demo, automated: refused tampered update, power cut mid-update,
# update, reboot into it, pick the previous system in the boot menu, rollback
qemu-test-update: update-demo
    image/qemu-test-update.py

# Delete build outputs (keeps the source cache and signing key)
clean-build:
    rm -rf build/work build/repo build/scratch-root build/demo-* build/probe-*

# --- Verification -----------------------------------------------------------

# Everything CI and every commit must pass
check: fmt-check clippy test lint
    @echo "just check: all green"

# Fail if any Rust code is not rustfmt-formatted
fmt-check:
    cargo fmt --all -- --check

# Lint every crate; warnings are errors
clippy:
    cargo clippy --workspace --all-targets --locked -- -D warnings

# Run every Rust test
test:
    cargo test --workspace --locked

# Repository invariants: placeholders, Apple names/assets, recipe justifications
lint:
    cargo run --quiet --locked -p ferncheck -- --root .

# --- Housekeeping -----------------------------------------------------------

# Format all Rust code
fmt:
    cargo fmt --all

# Build the flake's checks (hermetic build + tests of repo tooling)
nix-check:
    nix flake check

# Print the versions of the tools the devShell provides
doctor:
    @rustc --version
    @cargo --version
    @just --version
    @cmake --version | head -1
    @qemu-system-x86_64 --version | head -1
    @grub-mkimage --version
    @echo "OVMF_FV=${OVMF_FV:-unset}"
