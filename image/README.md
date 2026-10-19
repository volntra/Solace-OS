# image

Assembles the Solace system image and boots it in QEMU.

| File | What it does |
|---|---|
| `build.sh` | `just image`: installs `packages.txt` from the system repo into a staging root, makes the initramfs, writes the btrfs filesystem (`@snapshots/1` read-only, `@etc`, `@var`, `@users`), the FAT32 ESP (GRUB, our theme, sol-bootgen's grub.cfg, kernel + initramfs), and joins them into `build/image/solace.img` (GPT). Runs unprivileged and only writes regular files. |
| `packages.txt` | What goes into the image (dependencies come from the solver). |
| `boot-qemu.toml` | Boot settings for dev images: menu shown for 3 s, console mirrored to serial. |
| `qemu.sh` | `just qemu`: boots the image with OVMF (KVM when available). `--headless` for no window. |
| `qemu-test.py` | `just qemu-test`: boots headless, logs in over serial, checks `solctl`, the read-only `/usr`, the ESP and system volume mounts, `spk systems`, powers off. |
| `update-demo.sh` | `just update-demo`: builds the demo update (solace-base 0.1.1) and `build/demo/update.img`, an update disk with a good and a tampered repository. |
| `qemu-test-update.py` | `just qemu-test-update`: the Phase 3 test (refused tampered update, power cut mid-update, update, reboot into it, pick the previous system in the boot menu, `spk rollback`). |
| `qemulib.py` | Shared VM driver for the tests: serial console, QEMU monitor (screenshots, keys, power cut). |
| `base/` | Source of the `solace-base` package (os-release, factory `/etc`, console login units). |

Dev images have the root password `solace`. The live installer image
arrives in Phase 12.
