# Solace task runner. Run inside `nix develop` (or with direnv).

set shell := ["bash", "-euo", "pipefail", "-c"]

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
    target/release/spk build --all

# Run a hostile recipe that must fail to escape the build sandbox
sandbox-probe: spk
    @test -f build/keys/dev.key || target/release/spk key gen dev --dir build/keys
    target/release/spk build --force --recipes-dir tools/sandbox-probe --repo-dir build/probe-repo --work build/probe-work probe
    @grep -h "ok:" build/probe-work/probe/build.log

# Fill build/sources from the Nix binary cache where nixpkgs pins the same hash
seed-sources:
    tools/seed-sources-from-nix.sh build/sources

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
