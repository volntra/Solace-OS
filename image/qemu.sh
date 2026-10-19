#!/usr/bin/env bash
# Boots a Solace disk image in QEMU with UEFI firmware (OVMF).
#
#   image/qemu.sh [--headless] [disk]     (default build/image/solace.img)
#
# The console is mirrored to this terminal over the serial port; with
# --headless there is no window. Uses KVM when /dev/kvm is usable.
set -euo pipefail

headless=0
disk=build/image/solace.img
for a in "$@"; do
  case $a in
    --headless) headless=1 ;;
    -*) echo "usage: image/qemu.sh [--headless] [disk]" >&2; exit 2 ;;
    *) disk=$a ;;
  esac
done
[ -f "$disk" ] || { echo "$disk not found; run 'just image' first" >&2; exit 1; }
[ -n "${OVMF_FV:-}" ] || { echo "OVMF_FV unset (run inside 'nix develop')" >&2; exit 1; }

# Writable copy of the firmware variables, next to the disk.
vars=$(dirname "$disk")/OVMF_VARS.fd
if [ ! -f "$vars" ]; then
  cp "$OVMF_FV/OVMF_VARS.fd" "$vars"
  chmod u+w "$vars"
fi

if [ -r /dev/kvm ] && [ -w /dev/kvm ]; then
  accel=(-accel kvm -cpu host)
else
  echo "note: no /dev/kvm, using software emulation (much slower boots)" >&2
  accel=(-accel tcg -cpu max)
fi
if [ $headless = 1 ]; then
  display=(-display none -serial stdio -monitor none)
else
  display=(-device virtio-gpu-pci -serial mon:stdio)
fi

exec qemu-system-x86_64 -machine q35 -m 2G -smp 2 "${accel[@]}" \
  -drive if=pflash,format=raw,unit=0,readonly=on,file="$OVMF_FV/OVMF_CODE.fd" \
  -drive if=pflash,format=raw,unit=1,file="$vars" \
  -drive file="$disk",if=virtio,format=raw \
  -nic user,model=virtio-net-pci \
  "${display[@]}"
