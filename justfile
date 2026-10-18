# Solace task runner. Run inside `nix develop` (or with direnv).

set shell := ["bash", "-euo", "pipefail", "-c"]

# List recipes
default:
    @just --list --unsorted

# --- Current milestone demo -------------------------------------------------

# Run the demo for the current phase (see docs/STATUS.md)
demo: check
    @echo "Phase 0 demo passed: devShell + just check are green."

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
