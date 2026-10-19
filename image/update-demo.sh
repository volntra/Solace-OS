#!/usr/bin/env bash
# Builds the Phase 3 demo update and the "update disk" that carries it.
#
#   image/update-demo.sh [--out DIR]       (default build/demo)
#
# The update is solace-base with the next release number, reporting
# version 0.1.1 (os-release),
# so the boot menu and the login banner show which system is running. The
# update disk (<out>/update.img, FAT, no partition table) holds two
# repositories:
#   /good       the update, signed with the dev key
#   /tampered   the same, with one byte of the package changed
# Only regular files are written; no block device is opened.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
cd "$(dirname "$here")"

out=build/demo
[ "${1:-}" = --out ] && out=$2
SPK=${SPK:-target/release/spk}
[ -x "$SPK" ] || { echo "$SPK not found (just spk)" >&2; exit 1; }
for t in mformat mcopy mmd; do
  command -v "$t" >/dev/null || { echo "$t not found (run inside 'nix develop')" >&2; exit 1; }
done

rm -rf "$out"
mkdir -p "$out/recipes" "$out/repo"

# The update's source: solace-base with a newer version and release.
cp -r image/base "$out/base"
sed -i -e 's/^VERSION_ID=.*/VERSION_ID=0.1.1/' \
  -e 's/^VERSION=.*/VERSION="0.1.1 (Koi)"/' \
  -e 's/^PRETTY_NAME=.*/PRETTY_NAME="Solace 0.1.1 (Koi)"/' "$out/base/usr/lib/os-release"
mkdir -p "$out/recipes/solace-base"
release=$(sed -n 's/^release = \([0-9]*\)$/\1/p' recipes/solace-base/recipe.toml)
[ -n "$release" ] || { echo "no release in recipes/solace-base/recipe.toml" >&2; exit 1; }
next=$((release + 1))
sed -e "s/^release = $release\$/release = $next/" -e "s|^local = \"image/base\"|local = \"$out/base\"|" \
  recipes/solace-base/recipe.toml >"$out/recipes/solace-base/recipe.toml"
grep -q "^release = $next\$" "$out/recipes/solace-base/recipe.toml"
grep -q "^local = \"$out/base\"" "$out/recipes/solace-base/recipe.toml"

"$SPK" build --recipes-dir "$out/recipes" --repo-dir "$out/repo" --work "$out/work" \
  --toolchain native --dep-repo build/system/repo --dep-repo build/bootstrap/repo \
  --base glibc --base bash --base coreutils solace-base

# The update disk.
tampered=$out/tampered
cp -r "$out/repo" "$tampered"
pkg=$(ls "$tampered"/packages/solace-base-*.solpkg)
size=$(stat -c %s "$pkg")
printf 'X' | dd of="$pkg" bs=1 seek=$((size / 2)) conv=notrunc status=none
img=$out/update.img
truncate -s 64M "$img"
mformat -i "$img" -v UPDATE ::
mmd -i "$img" ::/good ::/tampered
mcopy -s -Q -i "$img" "$out/repo"/* ::/good/
mcopy -s -Q -i "$img" "$tampered"/* ::/tampered/
echo "update disk: $img (attach with: just qemu --update $img)"
