# Solace task runner. Run inside `nix develop` (or with direnv).

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
    resvg -w 512 assets/brand/solace-icon.svg assets/brand/solace-icon-512.png
    resvg -w 128 assets/brand/solace-mark.svg assets/brand/solace-mark-128.png
    resvg -w 512 assets/brand/solace-icon-glass.svg assets/brand/solace-icon-glass-512.png
    resvg -w 1350 build/brand/glass-preview.svg docs/design/img/glass-preview.png

# Render the wallpapers (assets/wallpapers/*.svg) to PNG under build/
wallpapers:
    python3 assets/wallpapers/gen-wallpapers.py
    mkdir -p build/wallpapers
    resvg -w 2560 assets/wallpapers/dusk.svg build/wallpapers/dusk.png
    resvg -w 2560 assets/wallpapers/dusk-blurred.svg build/wallpapers/dusk-blurred.png

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

# Assemble build/image/solace.img from the system repo (no real disks touched)
image *ARGS: spk
    image/build.sh {{ARGS}}

# Boot the image in QEMU (UEFI, our GRUB menu, console mirrored here)
qemu *ARGS:
    image/qemu.sh {{ARGS}}

# Boot headless, log in over serial, check solctl and the sealed /usr
qemu-test:
    image/qemu-test.py

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
    cargo run --quiet --locked -p solcheck -- --root .

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
