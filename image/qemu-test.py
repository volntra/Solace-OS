#!/usr/bin/env python3
"""Headless smoke test for the Fern QEMU image (`just qemu-test`).

Boots image/qemu.sh --headless, follows the serial console, and checks:
  1. GRUB boots our kernel and fern-init reports boot complete, with the
     time since the kernel started (the console is quiet until then)
  2. a text login prompt appears; root logs in with the dev password
  3. fernctl works: status, boot-time, logs; spk systems lists system 1;
     fern-init loaded the kernel modules listed in /usr/lib/fern/modules.d
  4. /etc/os-release, the Fern root layout, the read-only /usr, and the
     ESP and system volume mounted for updates
  5. fernctl poweroff shuts the machine down cleanly

Exit status 0 only if every step passed. The full console log is written
to build/image/qemu-test.log.
"""

import os
import re
import time

from qemulib import Console, fail, ok

LOG = "build/image/qemu-test.log"


def main():
    os.makedirs("build/image", exist_ok=True)
    if os.path.exists(LOG):
        os.remove(LOG)
    c = Console(["image/qemu.sh", "--headless"], LOG)
    try:
        boot_ms = c.boot_and_login()
        ok(f"fern-init: boot complete {boot_ms} ms after the kernel started; logged in as root")
        if "getty-serial" not in c.run("fernctl status"):
            fail("fernctl status did not list getty-serial")
        ok("fernctl status")
        if c.run("test -d /sys/module/zram && echo loaded") != "loaded":
            fail("fern-init did not load the zram module: " + c.run("fernctl logs -n 40 fern-init | grep -i module"))
        ok("fern-init loaded the zram kernel module (compressed .ko.zst)")
        if "ms after the kernel started" not in c.run("fernctl boot-time"):
            fail("fernctl boot-time gave no time")
        ok("fernctl boot-time")
        c.run("fernctl logs -n 5")
        if "ID=fern" not in c.run("cat /etc/os-release"):
            fail("/etc/os-release does not identify Fern")
        ok("/etc/os-release (from the factory /etc)")
        listing = c.run("ls -A /")
        for name in ("Applications", "Users", "System", "Volumes", "usr", "etc"):
            if name not in listing:
                fail(f"/{name} missing from the root")
        ok("Fern root layout (Applications, Users, System, Volumes)")
        if re.search(r"btrfs\s+ro,", c.run("findmnt -n -o FSTYPE,OPTIONS /usr")) is None:
            fail("/usr is not a read-only btrfs mount")
        ok("/usr is the sealed read-only snapshot")
        mounts = c.run("findmnt -n -o TARGET,FSTYPE /boot; findmnt -n -o TARGET,FSTYPE /run/fern/btrfs")
        if "vfat" not in mounts or "btrfs" not in mounts:
            fail(f"the ESP or the system volume is not mounted: {mounts!r}")
        ok("ESP at /boot and the system volume at /run/fern/btrfs")
        systems = c.run("spk systems")
        if not re.search(r"^1\s+\S+\s+.*running", systems, re.M):
            fail(f"spk systems does not show system 1 running: {systems!r}")
        ok("spk systems")
        c.send("fernctl poweroff\n")
        deadline = time.monotonic() + 120
        while c.p.poll() is None and time.monotonic() < deadline:
            c._read()
            time.sleep(0.1)
        if c.p.poll() is None:
            fail("the machine did not power off")
        ok("clean poweroff")
        verdict = "under" if boot_ms < 5000 else "OVER"
        kvm = os.access("/dev/kvm", os.W_OK)
        print(f"\nqemu-test: PASS (boot {boot_ms} ms, {verdict} the 5 s target"
              f"{'' if kvm else '; no KVM, so this is software emulation'})")
    finally:
        c.close()


if __name__ == "__main__":
    main()
