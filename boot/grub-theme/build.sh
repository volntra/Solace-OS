#!/usr/bin/env bash
# Builds the Solace GRUB theme and fonts.
#
#   build.sh <theme-out> <fonts-out> <Inter-4.1.zip> <assets-dir>
#
# Needs python3, resvg, and grub-mkfont. Output is data only (PNG, PF2,
# theme.txt). Used by recipes/solace-grub-theme and `just grub-theme`.
set -euo pipefail

theme=$1 fonts=$2 inter_zip=$3 assets=$4
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

mkdir -p "$theme" "$fonts"
python3 "$here/gen-theme.py" "$work/svg"
cp "$work/svg/theme.txt" "$theme/"
for f in "$work"/svg/*.svg; do
  resvg "$f" "$theme/$(basename "${f%.svg}").png"
done
resvg -w 1920 -h 1080 "$assets/wallpapers/dusk-blurred.svg" "$theme/background.png"
resvg -w 144 -h 144 "$assets/brand/solace-icon-glass.svg" "$theme/logo.png"

python3 - "$inter_zip" "$work" <<'PY'
import sys, zipfile
z = zipfile.ZipFile(sys.argv[1])
for n in ("extras/ttf/Inter-Regular.ttf", "extras/ttf/Inter-SemiBold.ttf", "LICENSE.txt"):
    with open(f"{sys.argv[2]}/{n.split('/')[-1]}", "wb") as f:
        f.write(z.read(n))
PY
grub-mkfont -s 16 -o "$fonts/inter-regular-16.pf2" "$work/Inter-Regular.ttf"
grub-mkfont -s 20 -o "$fonts/inter-regular-20.pf2" "$work/Inter-Regular.ttf"
grub-mkfont -s 24 -o "$fonts/inter-semibold-24.pf2" "$work/Inter-SemiBold.ttf"
cp "$work/LICENSE.txt" "$fonts/Inter-LICENSE.txt"
echo "theme: $(ls "$theme" | wc -l) files, fonts: $(ls "$fonts" | wc -l) files"
