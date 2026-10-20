# fern-bootgen inputs

`fern-bootgen --esp /boot --inventory systems.toml [--settings boot.toml]`

## Inventory (`systems.toml`)

Written by whoever knows the bootable systems: the image builder now,
spk's system transactions from Phase 3.

```toml
esp_uuid = "ABCD-1234"                              # FAT volume id of the ESP
root_uuid = "0e7b1b1c-2d33-4c6e-9a7e-1f2b3c4d5e6f"  # btrfs filesystem UUID
os_name = "Fern"                                  # default shown

[[system]]
id = "1"                       # [A-Za-z0-9._-]+ -> /boot/fern/1/
version = "0.1"
created = 1792368000           # unix seconds; newest boots first
subvol = "@snapshots/1"        # passed as fern.system=
kernel = "/path/to/vmlinuz"    # copied to the ESP
initramfs = "/path/to/initramfs.img"

[[other_os]]                   # optional, from os-prober
title = "Windows Boot Manager"
uuid = "1234-ABCD"
efi_path = "/EFI/Microsoft/Boot/bootmgfw.efi"
```

## Boot settings (`boot.toml`, the settings domain)

```toml
menu = "hidden"     # always | hidden (Shift or Esc shows it) | never
timeout = 5         # seconds, for menu = "always"
theme = true        # our theme, or plain text
verbose = false     # kernel/init messages instead of the splash
default = "1"       # system id; default: the newest
keep = 3            # previous systems kept bootable
other_os = true
console = []        # kernel consoles, e.g. ["tty0", "ttyS0,115200"]; last is /dev/console
```

## Kernel command line

Each entry boots `/fern/<id>/vmlinuz` with
`fern.root=UUID=<root_uuid> fern.system=<subvol> fern.esp=UUID=<esp_uuid> rw` plus
`quiet loglevel=3 ...` (graphical) or `fern.verbose loglevel=7` (verbose).
fern-init in the initramfs reads `fern.root`/`fern.system` and assembles the
root itself (it is not a single subvolume), so `rootflags=subvol=` is not
used.
