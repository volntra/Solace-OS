#!/usr/bin/env bash
# Boots a Fern disk image in QEMU with UEFI firmware (OVMF).
#
#   image/qemu.sh [--headless | --screen] [--update IMG] [disk]
#                                        (default disk build/image/fern.img)
#
# The console is mirrored to this terminal over the serial port; with
# --headless there is no window (only QEMU's default VGA, enough for the
# firmware and GRUB), with --screen fern-comp's GPU but no window (tests look
# at both through QEMU_MONITOR screenshots). --update
# attaches an update disk (image/update-demo.sh) read-only as /dev/vdb.
# QEMU_MONITOR=<path> opens the QEMU monitor on that Unix socket (tests use
# it for screenshots and key presses). Uses KVM when /dev/kvm is usable.
set -euo pipefail

headless=0
disk=build/image/fern.img
update=()
while [ $# -gt 0 ]; do
  case $1 in
    --headless) headless=1 ;;
    --screen) headless=2 ;;
    --update)
      [ -f "${2:-}" ] || { echo "update disk ${2:-} not found (just update-demo)" >&2; exit 1; }
      update=(-drive "file=$2,if=virtio,format=raw,readonly=on")
      shift ;;
    -*) echo "usage: image/qemu.sh [--headless | --screen] [--update IMG] [disk]" >&2; exit 2 ;;
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
# One GPU (QEMU would otherwise add a standard VGA card, and the firmware,
# GRUB and the screenshots would use that one): virtio-gpu with VGA
# compatibility for the firmware. Plus a keyboard and an absolute pointer
# (the window's mouse maps 1:1). With 3D on the host, -device virtio-vga-gl
# and -display gtk,gl=on give fern-comp accelerated rendering (virgl).
screen=(-vga none -device virtio-vga -device virtio-keyboard-pci -device virtio-tablet-pci)
if [ $headless = 1 ]; then
  display=(-display none -serial stdio "${monitor[@]}")
elif [ $headless = 2 ]; then
  display=(-display none "${screen[@]}" -serial stdio "${monitor[@]}")
elif [ -n "${QEMU_MONITOR:-}" ]; then
  display=("${screen[@]}" -serial stdio "${monitor[@]}")
else
  display=("${screen[@]}" -serial mon:stdio)
fi

exec qemu-system-x86_64 -machine q35 -m 2G -smp 2 "${accel[@]}" \
  -drive if=pflash,format=raw,unit=0,readonly=on,file="$OVMF_FV/OVMF_CODE.fd" \
  -drive if=pflash,format=raw,unit=1,file="$vars" \
  -drive file="$disk",if=virtio,format=raw \
  "${update[@]}" \
  -nic user,model=virtio-net-pci \
  "${display[@]}"
