#!/usr/bin/env bash
# Phase 1 milestone demo: build 10 recipes and install them into a scratch
# root with spk, then show that spk refuses what it must refuse.
#
# Run inside `nix develop` (via `just demo`). Writes only under build/.
set -euo pipefail

SPK=${SPK:-target/release/spk}
ROOT=build/scratch-root
REPO=build/repo
KEY=build/keys/dev

step() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
expect_fail() {
  local what=$1; shift
  if "$@" >/tmp/spk-demo-out 2>&1; then
    echo "FAIL: expected spk to refuse: $what" >&2
    cat /tmp/spk-demo-out >&2
    exit 1
  fi
  printf '  refused as expected (%s):\n    %s\n' "$what" "$(tail -1 /tmp/spk-demo-out)"
}

step "Signing key"
if [ ! -f "$KEY.key" ]; then
  "$SPK" key gen dev --dir build/keys
else
  echo "  using existing $KEY.key"
fi

step "Seeding the source cache from Nix (hash-checked; skips what it cannot match)"
tools/seed-sources-from-nix.sh build/sources 2>/dev/null | sed 's/^/  /'

step "Building every recipe (sandboxed, unprivileged, no network)"
"$SPK" build --all --repo-dir "$REPO" --key "$KEY.key"

step "Fresh scratch root that trusts only our dev key"
rm -rf "$ROOT"
"$SPK" --root "$ROOT" key trust "$KEY.pub"

step "Installing the 10 recipes' packages; the solver pulls in the split libraries"
"$SPK" --root "$ROOT" --repo "$REPO" install \
  zlib bzip2 xz lz4 zstd expat libffi pcre2 libxcrypt libarchive

step "What is installed"
"$SPK" --root "$ROOT" list

step "Queries"
"$SPK" --root "$ROOT" owns /usr/bin/bsdtar
"$SPK" --root "$ROOT" owns /usr/lib/liblzma.so.5
"$SPK" --root "$ROOT" info libarchive
echo "  files of lz4 (first 8):"
"$SPK" --root "$ROOT" files lz4 | head -8 | sed 's/^/    /'
"$SPK" --root "$ROOT" verify

step "Refusals"
expect_fail "removing a library others need" \
  "$SPK" --root "$ROOT" remove liblzma

tmp=build/demo-tamper
rm -rf "$tmp" && mkdir -p "$tmp"
pkg=$(ls "$REPO"/packages/lz4-*.solpkg)
cp "$pkg" "$pkg.sig" "$tmp/"
t="$tmp/$(basename "$pkg")"
printf '\x00' | dd of="$t" bs=1 seek=4000 conv=notrunc status=none
expect_fail "a package with one changed byte" \
  "$SPK" --root "$ROOT" install "$t"
rm -f "$t.sig"
expect_fail "an unsigned package" \
  "$SPK" --root "$ROOT" install "$t"

other=build/demo-untrusted-root
rm -rf "$other"
expect_fail "a root that trusts no keys" \
  "$SPK" --root "$other" --repo "$REPO" install zlib

expect_fail "an http:// repository" \
  "$SPK" --root "$ROOT" --repo http://example.org/repo install zlib

step "Removing libarchive (nothing depends on it)"
"$SPK" --root "$ROOT" remove libarchive
"$SPK" --root "$ROOT" history | tail -4

step "Phase 1 demo passed"
echo "  scratch root: $ROOT  ($(du -sh "$ROOT" | cut -f1))"
