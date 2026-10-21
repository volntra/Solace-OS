#!/usr/bin/env python3
"""The desktop shell on the image (`just qemu-test-shell`), the Phase 6 demo.

Boots image/qemu.sh --screen, logs in on the serial console (which reaches
the session's bus at /run/user/0/bus), and drives the real session with key
presses through the QEMU monitor, checking screenshots and the session's
services:
  1. the session started the bus, fern-settingsd, fern-indexd and the
     shell; the menu bar and the dock are on screen, over Grove
  2. Super+Space opens Search; "dark" finds Dark Mode; Return turns the
     desktop dark (menu bar, Grove at night) and the setting says so
  3. Print takes a screenshot: a PNG in ~/Pictures/Screenshots and a
     notification banner at the top right
  4. Search's "Control Center" and "Notification Center" open their panels;
     Escape closes them
  5. the indexer finds a file made on the console (org.fern.Search1)
  6. killing fern-shell: fern-comp starts it again and the menu bar is back
  7. the machine powers off cleanly

Screenshots and the console log are written to build/image/shell-*.png.
"""

import os
import time

from qemulib import Console, fail, ok, to_png

OUT = "build/image"
LOG = f"{OUT}/qemu-test-shell.log"
MONITOR = f"{OUT}/qemu-test-shell.monitor"
BUS = "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/0/bus"


class Shot:
    def __init__(self, c, name):
        ppm = f"{OUT}/shell-{name}.ppm"
        self.name = name
        self.w, self.h, self.px = c.screenshot(ppm)
        to_png(ppm, f"{OUT}/shell-{name}.png")
        os.remove(ppm)

    def pixel(self, x, y):
        i = (y * self.w + x) * 3
        return tuple(self.px[i : i + 3])

    def luma(self, x, y):
        r, g, b = self.pixel(x, y)
        return 0.2126 * r + 0.7152 * g + 0.0722 * b

    def mean_luma(self, x0, y0, x1, y1):
        pts = [(x, y) for y in range(y0, y1, 4) for x in range(x0, x1, 4)]
        return sum(self.luma(x, y) for x, y in pts) / max(len(pts), 1)


def changed(a, b, x0, y0, x1, y1):
    """Mean absolute luma difference over a region of two screenshots."""
    pts = [(x, y) for y in range(y0, y1, 4) for x in range(x0, x1, 4)]
    return sum(abs(a.luma(x, y) - b.luma(x, y)) for x, y in pts) / max(len(pts), 1)


def wait_for(c, name, test, what, timeout=180):
    deadline = time.monotonic() + timeout
    while True:
        shot = Shot(c, name)
        if test(shot):
            return shot
        if time.monotonic() > deadline:
            logs = c.run("fernctl logs -n 300 fern-comp | grep -v 'MESA\\|INFO' | tail -40", "fern-comp's log")
            fail(f"timed out waiting for {what} (last screenshot: {OUT}/shell-{name}.png)\n{logs}")
        c._read()
        time.sleep(1.5)


def typed(c, text):
    for ch in text:
        c.key("spc" if ch == " " else ch)
        time.sleep(0.05)


# The image has no procps: processes are found in /proc.
def pids(c, comm):
    out = c.run(
        f'for p in /proc/[0-9]*; do [ "$(cat $p/comm 2>/dev/null)" = {comm} ] && echo ${{p#/proc/}}; done; echo end',
        f"{comm}'s pid",
    ).split()
    return [w for w in out if w.isdigit()]


def menu_bar_light(s):
    return s.mean_luma(s.w // 3, 6, 2 * s.w // 3, 22) > 150


def menu_bar_dark(s):
    return s.mean_luma(s.w // 3, 6, 2 * s.w // 3, 22) < 70


def main():
    os.makedirs(OUT, exist_ok=True)
    if os.path.exists(LOG):
        os.remove(LOG)
    c = Console(["image/qemu.sh", "--screen"], LOG, monitor=MONITOR)
    try:
        c.boot_and_login()
        ok("booted and logged in on the serial console")
        deadline = time.monotonic() + 300
        while "shell started" not in c.run("fernctl logs -n 200 fern-comp", "fern-comp's log"):
            if time.monotonic() > deadline:
                fail("fern-comp never started the shell: " + c.run("fernctl logs -n 60 fern-comp"))
            time.sleep(3)
        for comm in ["dbus-daemon", "fern-settingsd", "fern-indexd", "fern-shell"]:
            if not pids(c, comm):
                fail(f"{comm} is not running: " + c.run("fernctl logs -n 60 fern-comp"))
        theme = c.run(f"{BUS} fern-settings get appearance theme", "the theme setting").strip()
        if "light" not in theme:
            fail(f"fern-settings could not read the theme: {theme}")
        ok("the session runs the bus, fern-settingsd, fern-indexd and fern-shell; settings answer")

        desk = wait_for(c, "desktop", menu_bar_light, "the menu bar")
        # The dock: a lighter panel at the bottom centre than the forest
        # beside it.
        dock = desk.mean_luma(desk.w // 2 - 60, desk.h - 50, desk.w // 2 + 60, desk.h - 30)
        forest = desk.mean_luma(40, desk.h - 50, 160, desk.h - 30)
        if dock < forest + 25:
            fail(f"no dock at the bottom (dock {dock:.0f}, forest {forest:.0f})")
        ok("the menu bar and the dock are on screen, over Grove")

        # Search, by keyboard: "dark", Return.
        c.key("meta_l-spc")
        before = desk
        search = wait_for(
            c, "search",
            lambda s: changed(before, s, s.w // 3, s.h // 6, 2 * s.w // 3, s.h // 4) > 20,
            "the Search panel",
        )
        time.sleep(5)
        typed(c, "dark")
        time.sleep(15)
        Shot(c, "search-dark")
        c.key("ret")
        sky_day = desk.mean_luma(desk.w // 4, 60, desk.w // 2, 140)
        # The compositor decodes the night picture before it fades to it
        # (slow in an emulated VM): wait for both.
        wait_for(
            c, "dark",
            lambda s: menu_bar_dark(s) and s.mean_luma(s.w // 4, 60, s.w // 2, 140) < sky_day - 30,
            "the dark menu bar and Grove at night", timeout=300,
        )
        theme = c.run(f"{BUS} fern-settings get appearance theme", "the theme setting").strip()
        if "dark" not in theme:
            fail(f"the setting did not change: {theme}")
        ok("Super+Space, \"dark\", Return: dark mode, Grove at night, and the setting says dark")
        del search

        # A screenshot with Print: a banner at the top right, and the file.
        # Without KVM (software emulation, software GL) the capture alone
        # takes a minute or more; banners stay 180 s so the test sees one.
        c.run(f"{BUS} fern-settings set notifications banner_seconds 180", "a longer banner time")
        quiet = Shot(c, "before-print")
        c.key("print")
        wait_for(
            c, "banner",
            lambda s: changed(quiet, s, s.w - 330, 40, s.w - 30, 110) > 15,
            "the notification banner", timeout=420,
        )
        if "Screenshot" not in c.run("ls /root/Pictures/Screenshots 2>/dev/null; echo .", "the screenshots folder"):
            fail("a banner, but no screenshot file")
        ok("Print: a PNG in ~/Pictures/Screenshots and a notification banner")
        wait_for(
            c, "banner-gone",
            lambda s: changed(quiet, s, s.w - 330, 40, s.w - 30, 200) < 6,
            "the banner to go", timeout=300,
        )

        # The control center and the notification center, from Search.
        for name, words in [("control-center", "control"), ("notification-center", "notification")]:
            calm = Shot(c, f"before-{name}")
            c.key("meta_l-spc")
            # Typing before Search holds the keyboard would be lost.
            wait_for(
                c, f"{name}-search",
                lambda s: changed(calm, s, s.w // 3, s.h // 6, 2 * s.w // 3, s.h // 4) > 20,
                "the Search panel", timeout=300,
            )
            time.sleep(5)
            typed(c, words)
            time.sleep(15)
            c.key("ret")
            wait_for(
                c, name,
                lambda s: changed(calm, s, s.w - 300, 120, s.w - 40, 260) > 12,
                name.replace("-", " "), timeout=300,
            )
            c.key("esc")
            wait_for(
                c, f"{name}-closed",
                lambda s: changed(calm, s, s.w - 300, 120, s.w - 40, 260) < 6,
                f"the {name.replace('-', ' ')} to close",
                timeout=180,
            )
        ok("Search opens the Control Center and the Notification Center; Escape closes them")

        # The indexer follows the home folder.
        c.run("mkdir -p /root/Documents && echo tent > '/root/Documents/Trip notes.txt'", "a document")
        deadline = time.monotonic() + 60
        while True:
            out = c.run(
                f"{BUS} dbus-send --session --print-reply --dest=org.fern.Search /org/fern/Search "
                "org.fern.Search1.Query string:trip uint32:5",
                "a file search",
            )
            if "Trip notes.txt" in out:
                break
            if time.monotonic() > deadline:
                fail(f"the indexer did not find the new file: {out}")
            time.sleep(1)
        ok("the indexer found a file made a moment before (org.fern.Search1)")

        # The shell is restarted if it dies.
        old = pids(c, "fern-shell")
        c.run(f"kill {old[0]}", "killing fern-shell")
        deadline = time.monotonic() + 120
        new = pids(c, "fern-shell")
        while not new or new == old:
            if time.monotonic() > deadline:
                fail("fern-comp did not restart the shell: " + c.run("fernctl logs -n 40 fern-comp"))
            time.sleep(1)
            new = pids(c, "fern-shell")
        wait_for(c, "after-restart", menu_bar_dark, "the menu bar after the restart", timeout=300)
        ok(f"fern-shell killed: fern-comp started a new one (pid {new[0]}), still dark")

        c.send("fernctl poweroff\n")
        deadline = time.monotonic() + 120
        while c.p.poll() is None and time.monotonic() < deadline:
            c._read()
            time.sleep(0.1)
        if c.p.poll() is None:
            fail("the machine did not power off")
        ok("clean poweroff")
        print(f"\nqemu-test-shell: PASS (screenshots in {OUT}/shell-*.png)")
    finally:
        c.close()


if __name__ == "__main__":
    main()
