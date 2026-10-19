#!/usr/bin/env bash
# Builds a bootable Solace disk image for QEMU from signed packages.
#
#   image/build.sh [--repo DIR]... [--out DIR]
#
# Output: <out>/solace.img, a GPT disk image with
#   p1  ESP (FAT32): GRUB, our theme, grub.cfg and /sol/1/{vmlinuz,initramfs.img}
#   p2  btrfs: @snapshots/1 (the sealed system, read-only), @etc, @var, @users
#
# Only regular files are written; no block device is ever opened
# (CLAUDE.md rule 8). Runs unprivileged: it re-executes itself in a user
# namespace where the caller is uid 0, so every file in the image is owned
# by root without real root (packages carry no other owners).
#
# Environment:
#   SPK              spk binary (default target/release/spk)
#   SOL_KEY          trusted public key (default build/keys/dev.pub)
#   SOL_ROOT_PASSWORD  dev root password (default "solace"); dev images only
#   ESP_MIB ROOT_MIB partition sizes (default 2048 and 4096)
#   BOOT_TOML        boot settings for sol-bootgen (default image/boot-qemu.toml)
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
repo_root=$(dirname "$here")
cd "$repo_root"

if [ "$(id -u)" != 0 ]; then
  exec unshare --user --map-root-user -- "$0" "$@"
fi

repos=() out=build/image
while [ $# -gt 0 ]; do
  case $1 in
    --repo) repos+=("$2"); shift 2 ;;
    --out) out=$2; shift 2 ;;
    *) echo "usage: image/build.sh [--repo DIR]... [--out DIR]" >&2; exit 2 ;;
  esac
done
[ ${#repos[@]} -gt 0 ] || repos=(build/system/repo)

SPK=${SPK:-target/release/spk}
SOL_KEY=${SOL_KEY:-build/keys/dev.pub}
ESP_MIB=${ESP_MIB:-2048}
ROOT_MIB=${ROOT_MIB:-4096}
BOOT_TOML=${BOOT_TOML:-image/boot-qemu.toml}
password=${SOL_ROOT_PASSWORD:-solace}

step() { printf '\n\033[1m==> %s\033[0m\n' "$*"; }
die() { echo "image/build.sh: $*" >&2; exit 1; }

for t in mkfs.btrfs mkfs.fat mcopy sfdisk openssl; do
  command -v "$t" >/dev/null || die "$t not found (run inside 'nix develop')"
done
[ -x "$SPK" ] || die "$SPK not found (just spk)"

work=$out/work
rm -rf "$work"
mkdir -p "$work"

step "Installing the system into a staging root"
root=$work/root
"$SPK" --root "$root" key trust "$SOL_KEY"
repo_args=()
for r in "${repos[@]}"; do repo_args+=(--repo "$r"); done
mapfile -t pkgs < <(sed -e 's/#.*//' -e '/^[[:space:]]*$/d' image/packages.txt)
"$SPK" --root "$root" "${repo_args[@]}" install "${pkgs[@]}"
extra=$(find "$root" -mindepth 1 -maxdepth 1 ! -name usr -printf '%f ')
[ -z "$extra" ] || die "packages wrote outside usr/: $extra"
sys=$root/usr

# Our own tools from the packages just installed (static binaries).
for b in sbin/sol-bootgen sbin/sol-mkinitramfs lib/sol/sol-init lib/sol/boot/vmlinuz; do
  [ -f "$sys/$b" ] || die "missing /usr/$b in the installed system"
done
# Read (never source) the system's os-release: package content is data.
osr() { sed -n "s/^$1=\"\{0,1\}\([^\"]*\)\"\{0,1\}\$/\1/p" "$sys/lib/os-release"; }
NAME=$(osr NAME) VERSION_ID=$(osr VERSION_ID)
[ -n "$NAME" ] && [ -n "$VERSION_ID" ] || die "os-release lacks NAME or VERSION_ID"

step "initramfs (sol-init as /init)"
"$sys/sbin/sol-mkinitramfs" "$sys/lib/sol/sol-init" "$work/initramfs.img"

step "btrfs subvolumes"
fs=$work/fs
mkdir -p "$fs/@snapshots" "$fs/@var" "$fs/@users"
mv "$sys" "$fs/@snapshots/1"
sys=$fs/@snapshots/1
cp -a "$sys/share/factory/etc" "$fs/@etc"
hash=$(openssl passwd -6 -stdin <<<"$password")
sed -i "s|^root:[^:]*:|root:$hash:|" "$fs/@etc/shadow"
mkdir -p "$fs/@var"/{log,lib,cache,spool,tmp}
chmod 1777 "$fs/@var/tmp"
root_uuid=$(cat /proc/sys/kernel/random/uuid)
truncate -s "${ROOT_MIB}M" "$work/root.img"
mkfs.btrfs -q -U "$root_uuid" -L Solace --rootdir "$fs" --compress zstd:3 \
  --subvol rw:@snapshots --subvol ro:@snapshots/1 \
  --subvol rw:@etc --subvol rw:@var --subvol rw:@users \
  "$work/root.img"

step "ESP: GRUB, theme, grub.cfg, kernel"
esp=$work/esp
mkdir -p "$esp/EFI/BOOT" "$esp/grub"
cp -r "$sys/share/sol/grub/themes" "$sys/share/sol/grub/fonts" "$esp/grub/"
[ -f "$sys/lib/sol/boot/grubx64.efi" ] || die "no grubx64.efi: is the grub package installed?"
cp "$sys/lib/sol/boot/grubx64.efi" "$esp/EFI/BOOT/BOOTX64.EFI"
serial=$(od -An -N4 -tx4 /dev/urandom | tr -d ' \n' | tr a-f A-F)
esp_uuid=${serial:0:4}-${serial:4:4}
cat >"$work/systems.toml" <<EOF
esp_uuid = "$esp_uuid"
root_uuid = "$root_uuid"
os_name = "$NAME"

[[system]]
id = "1"
version = "$VERSION_ID"
created = ${SOURCE_DATE_EPOCH:-$(date +%s)}
subvol = "@snapshots/1"
kernel = "$sys/lib/sol/boot/vmlinuz"
initramfs = "$work/initramfs.img"
EOF
"$sys/sbin/sol-bootgen" --esp "$esp" --inventory "$work/systems.toml" --settings "$BOOT_TOML"
truncate -s "${ESP_MIB}M" "$work/esp.img"
mkfs.fat -F 32 -i "$serial" -n SOLACE "$work/esp.img" >/dev/null
mcopy -s -Q -i "$work/esp.img" "$esp"/* ::/

step "GPT disk image"
disk=$out/solace.img
rm -f "$disk"
truncate -s "$((1 + ESP_MIB + ROOT_MIB + 1))M" "$disk"
sfdisk -q "$disk" <<EOF
label: gpt
start=2048, size=$((ESP_MIB * 2048)), type=C12A7328-F81F-11D2-BA4B-00A0C93EC93B, name="EFI system"
start=$((2048 + ESP_MIB * 2048)), size=$((ROOT_MIB * 2048)), type=4F68BCE3-E8CD-4DB1-96E7-FBCAF984B709, name="Solace"
EOF
dd if="$work/esp.img" of="$disk" bs=1M seek=1 conv=notrunc,sparse status=none
dd if="$work/root.img" of="$disk" bs=1M seek=$((1 + ESP_MIB)) conv=notrunc,sparse status=none

cat >"$out/image.toml" <<EOF
# Written by image/build.sh
disk = "solace.img"
version = "$VERSION_ID"
esp_uuid = "$esp_uuid"
root_uuid = "$root_uuid"
dev_root_password = true
EOF
rm -rf "$work"
step "Done: $disk ($(du -h --apparent-size "$disk" | cut -f1) sparse, $(du -h "$disk" | cut -f1) used)"
echo "Boot it with: just qemu   (dev login: root / $password)"
