#!/usr/bin/env bash
# Seeds spk's source cache from the Nix binary cache.
#
# For each recipe whose nixpkgs package (same name) pins the *same* sha256,
# fetch that source through Nix and store it as build/sources/<sha256>.
# spk re-verifies the hash before every use, so this can only save a
# download, never change what gets built.
#
# Useful offline, behind restrictive proxies (the cloud dev container cannot
# reach sourceware.org), or to avoid hammering upstream mirrors.
set -euo pipefail

cache="${1:-build/sources}"
mkdir -p "$cache"
for r in recipes/*/recipe.toml; do
  name=$(basename "$(dirname "$r")")
  want=$(sed -n 's/^sha256 = "\([0-9a-f]\{64\}\)"/\1/p' "$r" | head -1)
  [ -n "$want" ] || continue
  if [ -f "$cache/$want" ]; then
    echo "have    $name"
    continue
  fi
  sri=$(nix eval --raw --inputs-from . "nixpkgs#$name.src.outputHash" 2>/dev/null) || {
    echo "skip    $name (not in nixpkgs)"; continue; }
  have=$(nix hash convert --hash-algo sha256 --to base16 "$sri")
  if [ "$have" != "$want" ]; then
    echo "skip    $name (nixpkgs pins a different source)"
    continue
  fi
  out=$(nix build --no-link --print-out-paths --inputs-from . "nixpkgs#$name.src")
  install -m 0644 "$out" "$cache/$want.part"
  got=$(sha256sum "$cache/$want.part" | cut -d' ' -f1)
  if [ "$got" != "$want" ]; then
    rm -f "$cache/$want.part"
    echo "error   $name: hash mismatch from nix ($got)" >&2
    exit 1
  fi
  mv "$cache/$want.part" "$cache/$want"
  echo "seeded  $name"
done
