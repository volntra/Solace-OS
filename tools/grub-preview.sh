#!/usr/bin/env bash
# Boots a throwaway ESP with our GRUB theme in QEMU (OVMF) and saves a
# screenshot of the real boot menu. Development aid; writes only build/.
#
#   [GRUB_EFI=grubx64.efi] tools/grub-preview.sh [out.ppm]
set -euo pipefail
out=${1:-build/grub-preview/menu.ppm}
dir=build/grub-preview
rm -rf "$dir" && mkdir -p "$dir/esp/EFI/BOOT" "$dir/esp/grub/themes" "$dir/esp/grub/fonts" "$dir/esp/sol/1" "$dir/esp/sol/2"

# Theme + fonts (built by `just grub-theme`).
cp -r build/grub-theme/theme "$dir/esp/grub/themes/solace"
cp build/grub-theme/fonts/*.pf2 "$dir/esp/grub/fonts/"

# Two fake systems and one other OS, so the menu has something to show.
echo k > "$dir/k"; echo i > "$dir/i"
cat > "$dir/inventory.toml" <<INV
esp_uuid = "ABCD-1234"
root_uuid = "0e7b1b1c-2d33-4c6e-9a7e-1f2b3c4d5e6f"
[[system]]
id = "2"
version = "0.1"
created = 1792454400
subvol = "@snapshots/2"
kernel = "$dir/k"
initramfs = "$dir/i"
[[system]]
id = "1"
version = "0.1"
created = 1792368000
subvol = "@snapshots/1"
kernel = "$dir/k"
initramfs = "$dir/i"
[[other_os]]
title = "Windows"
uuid = "1234-ABCD"
efi_path = "/EFI/Microsoft/Boot/bootmgfw.efi"
INV
printf 'menu = "always"\ntimeout = 30\n' > "$dir/boot.toml"
cargo run -q -p sol-bootgen --bin sol-bootgen -- --esp "$dir/esp" --inventory "$dir/inventory.toml" --settings "$dir/boot.toml"

# GRUB_EFI=<grubx64.efi> previews a given GRUB binary (e.g. our package's).
if [ -n "${GRUB_EFI:-}" ]; then
  cp "$GRUB_EFI" "$dir/esp/EFI/BOOT/BOOTX64.EFI"
else
  grub-mkimage -d "$GRUB_EFI_LIB" -O x86_64-efi -p /grub -o "$dir/esp/EFI/BOOT/BOOTX64.EFI" \
    part_gpt part_msdos fat normal configfile search search_fs_uuid all_video \
    gfxterm gfxmenu png font echo linux gzio chain keystatus test sleep
fi

# A FAT image with volume id ABCD-1234, filled with mtools (no root).
img="$dir/esp.img"
truncate -s 64M "$img"
mkfs.vfat -n SOLACE -i ABCD1234 "$img" >/dev/null
(cd "$dir/esp" && find . -mindepth 1 -type d | sort | while read -r d; do mmd -i "../esp.img" "::${d#.}"; done)
(cd "$dir/esp" && find . -type f | sort | while read -r f; do mcopy -i "../esp.img" "$f" "::${f#.}"; done)

cp "$OVMF_FV/OVMF_VARS.fd" "$dir/vars.fd"
mon="$dir/monitor.sock"
qemu-system-x86_64 -machine q35 -m 512 -display none -vga virtio \
  -drive if=pflash,format=raw,readonly=on,file="$OVMF_FV/OVMF_CODE.fd" \
  -drive if=pflash,format=raw,file="$dir/vars.fd" \
  -drive format=raw,file="$img" \
  -monitor unix:"$mon",server,nowait &
qemu=$!
trap 'kill $qemu 2>/dev/null || true' EXIT
for _ in $(seq 1 60); do [ -S "$mon" ] && break; sleep 0.25; done
sleep "${PREVIEW_WAIT:-12}"
python3 - "$mon" "$(realpath -m "$out")" <<'PY'
import socket, sys, time
s = socket.socket(socket.AF_UNIX); s.connect(sys.argv[1])
s.recv(4096)
s.sendall(f"screendump {sys.argv[2]}\n".encode()); time.sleep(1)
s.sendall(b"quit\n"); time.sleep(0.5)
PY
# PPM -> PNG (stdlib only).
python3 - "$out" "${out%.ppm}.png" <<'PY'
import struct, sys, zlib
data = open(sys.argv[1], "rb").read()
parts = data.split(maxsplit=4)
w, h, px = int(parts[1]), int(parts[2]), parts[4]
rows = b"".join(b"\0" + px[y * w * 3:(y + 1) * w * 3] for y in range(h))
def chunk(t, d): return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))
png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)) \
    + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b"")
open(sys.argv[2], "wb").write(png)
PY
echo "screenshot: ${out%.ppm}.png"
