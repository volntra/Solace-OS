#!/usr/bin/env bash
# Seeds spk's source cache from the Nix binary cache.
#
# Every [[source]] sha256 in recipes/ and bootstrap/recipes/ is matched
# against the sources nixpkgs pins (tools/nix-source-attrs.txt). Where the
# hashes are identical, the file is fetched through Nix and stored as
# build/sources/<sha256>. spk re-verifies the hash before every use, so this
# can only save a download, never change what gets built.
#
# Useful offline, behind restrictive proxies (the cloud dev container cannot
# reach GNU, kernel.org, or sourceware mirrors), or to spare upstream mirrors.
set -euo pipefail

cache="${1:-build/sources}"
mkdir -p "$cache"

declare -A want=()
while read -r h; do want[$h]=1; done < <(
  grep -rhoE '^sha256 = "[0-9a-f]{64}"' recipes bootstrap 2>/dev/null | grep -oE '[0-9a-f]{64}' | sort -u)

missing=0
for h in "${!want[@]}"; do [ -f "$cache/$h" ] || missing=$((missing + 1)); done
if [ "$missing" = 0 ]; then
  echo "all ${#want[@]} sources already cached"
  exit 0
fi

grep -vE '^\s*(#|$)' tools/nix-source-attrs.txt | while read -r attr; do
  sri=$(nix eval --raw --inputs-from . "nixpkgs#$attr.src.outputHash" 2>/dev/null) || continue
  hex=$(nix hash convert --hash-algo sha256 --to base16 "$sri" 2>/dev/null) || continue
  [ -n "${want[$hex]:-}" ] || continue
  if [ -f "$cache/$hex" ]; then echo "have    $attr"; continue; fi
  out=$(nix build --no-link --print-out-paths --inputs-from . "nixpkgs#$attr.src" 2>/dev/null) || {
    echo "skip    $attr (not in the binary cache)"; continue; }
  [ -f "$out" ] || { echo "skip    $attr (source is a directory)"; continue; }
  install -m 0644 "$out" "$cache/$hex.part"
  got=$(sha256sum "$cache/$hex.part" | cut -d' ' -f1)
  if [ "$got" != "$hex" ]; then
    rm -f "$cache/$hex.part"
    echo "error   $attr: hash mismatch from nix ($got)" >&2
    exit 1
  fi
  mv "$cache/$hex.part" "$cache/$hex"
  echo "seeded  $attr"
done
