#!/usr/bin/env bash
# Boots a Solace disk image in QEMU with UEFI firmware (OVMF).
#
#   image/qemu.sh [--headless] [--update IMG] [disk]
#                                        (default disk build/image/solace.img)
#
# The console is mirrored to this terminal over the serial port; with
# --headless there is no window. --update attaches an update disk
# (image/update-demo.sh) read-only as /dev/vdb. QEMU_MONITOR=<path> opens
# the QEMU monitor on that Unix socket (tests use it for screenshots and
# key presses). Uses KVM when /dev/kvm is usable.
set -euo pipefail

headless=0
disk=build/image/solace.img
update=()
while [ $# -gt 0 ]; do
  case $1 in
    --headless) headless=1 ;;
    --update)
      [ -f "${2:-}" ] || { echo "update disk ${2:-} not found (just update-demo)" >&2; exit 1; }
      update=(-drive "file=$2,if=virtio,format=raw,readonly=on")
      shift ;;
    -*) echo "usage: image/qemu.sh [--headless] [--update IMG] [disk]" >&2; exit 2 ;;
    *) disk=$1 ;;
  esac
  shift
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
monitor=(-monitor none)
[ -n "${QEMU_MONITOR:-}" ] && monitor=(-monitor "unix:$QEMU_MONITOR,server,nowait")
if [ $headless = 1 ]; then
  display=(-display none -serial stdio "${monitor[@]}")
elif [ -n "${QEMU_MONITOR:-}" ]; then
  display=(-device virtio-gpu-pci -serial stdio "${monitor[@]}")
else
  display=(-device virtio-gpu-pci -serial mon:stdio)
fi

exec qemu-system-x86_64 -machine q35 -m 2G -smp 2 "${accel[@]}" \
  -drive if=pflash,format=raw,unit=0,readonly=on,file="$OVMF_FV/OVMF_CODE.fd" \
  -drive if=pflash,format=raw,unit=1,file="$vars" \
  -drive file="$disk",if=virtio,format=raw \
  "${update[@]}" \
  -nic user,model=virtio-net-pci \
  "${display[@]}"
