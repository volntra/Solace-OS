#!/usr/bin/env python3
"""Phase 3 test: system updates and rollback (`just qemu-test-update`).

Works on a copy of build/image/fern.img with the update disk from
image/update-demo.sh (fern-base 0.1.1). Five boots:

  A  system 1: a tampered update is refused (no snapshot left, boot menu
     unchanged); a good update is started and the power is cut after the
     new system is written, before it is sealed (SPK_TEST_PAUSE)
  B  system 1 still boots; the update succeeds and the half-made snapshot
     is gone; system 2 is the next boot
  C  system 2 boots by default and reports 0.1.1
  D  the previous system is picked in the boot menu (down, Enter): system
     1 boots and reports 0.1; `spk rollback` makes it the default
  E  system 1 boots by default

Exit status 0 only if every step passed. Console log:
build/image/qemu-test-update.log; boot menu screenshot:
build/image/update-menu.png.
"""

import os
import re
import subprocess
import time

from qemulib import Console, fail, ok, to_png

LOG = "build/image/qemu-test-update.log"
DISK = "build/image/update-test.img"
UPDATE = "build/demo/update.img"
MONITOR = "build/image/update-test.monitor"
SHOT = "build/image/update-test.ppm"


def start():
    return Console(["image/qemu.sh", "--headless", "--update", UPDATE, DISK], LOG, MONITOR)


def mount_update(c):
    c.run("mkdir -p /Volumes/Update && mount -t vfat -o ro /dev/vdb /Volumes/Update")


def booted(c):
    return c.run("cat /run/fern/booted")


def version(c):
    m = re.search(r"VERSION_ID=(\S+)", c.run("cat /usr/lib/os-release"))
    return m.group(1) if m else "?"


def snapshots(c):
    return c.run("ls /run/fern/btrfs/@snapshots").split()


def is_boot_menu(w, h, px):
    """Our boot menu is the grove wallpaper: mostly forest green, not the
    firmware's black."""
    step = 3 * 997
    sample = [px[i : i + 3] for i in range(0, len(px) - 3, step)]
    green = sum(1 for p in sample if len(p) == 3 and p[1] > 50 and p[1] > p[0] and p[1] >= p[2])
    return green > len(sample) * 0.6


def pick_previous_in_boot_menu(c):
    """Waits for the boot menu on screen, then presses down and Enter."""
    deadline = time.monotonic() + 300
    while time.monotonic() < deadline:
        c._read()
        if is_boot_menu(*c.screenshot(SHOT)):
            to_png(SHOT, "build/image/update-menu.png")
            c.key("down")
            time.sleep(0.5)
            c.key("ret")
            return
        time.sleep(0.3)
    fail("the boot menu never appeared")


def reboot(c):
    c.send("fernctl reboot\n")


def main():
    if not os.path.exists(UPDATE):
        fail(f"{UPDATE} missing: run `just update-demo` first")
    os.makedirs("build/image", exist_ok=True)
    for f in (LOG, DISK):
        if os.path.exists(f):
            os.remove(f)
    subprocess.run(["cp", "--sparse=always", "build/image/fern.img", DISK], check=True)

    # A: refuse a tampered update, then lose power mid-update.
    c = start()
    try:
        c.boot_and_login()
        if booted(c) != "@snapshots/1":
            fail("the first boot is not system 1")
        mount_update(c)
        menu_before = c.run("sha256sum /boot/grub/grub.cfg")
        out = c.run("spk --repo /Volumes/Update/tampered upgrade; echo rc=$?", timeout=600)
        if "rc=0" in out or not re.search(r"verif|signature|sha256|mismatch", out, re.I):
            fail(f"the tampered update was not refused: {out!r}")
        if snapshots(c) != ["1"] or c.run("sha256sum /boot/grub/grub.cfg") != menu_before:
            fail("a refused update left a snapshot or changed the boot menu")
        ok("A: tampered update refused; no snapshot left, boot menu unchanged")
        # Stop the transaction once the new system is written but not yet
        # sealed or in the boot menu, and cut the power there.
        c.send("SPK_TEST_PAUSE=before-seal spk --repo /Volumes/Update/good upgrade\n")
        c.expect(rb"spk: test pause before-seal", "the update to reach the seal", 900)
        c.kill()
        ok("A: power cut after the new system was written, before it was sealed")
    finally:
        c.close()

    # B: the old system still boots; the update goes through on retry.
    c = start()
    try:
        c.boot_and_login()
        if booted(c) != "@snapshots/1":
            fail("after the power cut, system 1 did not boot")
        left = snapshots(c)
        ok(f"B: system 1 still boots after the power cut (snapshots on disk: {' '.join(left)})")
        mount_update(c)
        out = c.run("spk --repo /Volumes/Update/good upgrade; echo rc=$?", timeout=900)
        if "rc=0" not in out or "system 2 is ready" not in out:
            fail(f"the update failed: {out!r}")
        if snapshots(c) != ["1", "2"]:
            fail(f"unexpected snapshots after the update: {snapshots(c)}")
        sys_list = c.run("spk systems")
        if not re.search(r"^2\s+0\.1\.1\s.*next boot", sys_list, re.M):
            fail(f"system 2 is not the next boot: {sys_list!r}")
        ok("B: update applied as system 2 (half-made snapshot cleaned up); next boot: 2")
        reboot(c)

        # C: the new system boots by default.
        c.boot_and_login()
        if booted(c) != "@snapshots/2" or version(c) != "0.1.1":
            fail(f"system 2 did not boot by default ({booted(c)}, {version(c)})")
        ok("C: rebooted into system 2 (Fern 0.1.1)")

        # D: pick the previous system in the boot menu.
        reboot(c)
        pick_previous_in_boot_menu(c)
        c.boot_and_login()
        if booted(c) != "@snapshots/1" or version(c) != "0.1":
            fail(f"picking the previous system did not boot system 1 ({booted(c)}, {version(c)})")
        ok("D: picked the previous system in the boot menu: system 1 (Fern 0.1)")
        out = c.run("spk rollback")
        if "system 1 starts on the next boot" not in out:
            fail(f"spk rollback: {out!r}")
        ok("D: spk rollback made system 1 the default")
        reboot(c)

        # E: the rollback holds.
        c.boot_and_login()
        if booted(c) != "@snapshots/1":
            fail("after spk rollback, system 1 did not boot by default")
        ok("E: system 1 boots by default after the rollback")
        print(c.run("spk systems"))
        c.send("fernctl poweroff\n")
        deadline = time.monotonic() + 120
        while c.p.poll() is None and time.monotonic() < deadline:
            c._read()
            time.sleep(0.1)
        print("\nqemu-test-update: PASS")
    finally:
        c.close()


if __name__ == "__main__":
    main()
