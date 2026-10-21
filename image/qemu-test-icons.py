#!/usr/bin/env python3
"""Adaptive icons on the image (`just qemu-test-icons`), the Phase 7 demo.

Boots image/qemu.sh --screen, logs in on the serial console (which reaches
the session's bus at /run/user/0/bus) and drives the icon settings through
the real settings daemon, checking screenshots of the dock:
  1. the session is up; the dock shows the Downloads and Trash icons
  2. each of the five icon modes (Default, Dark, Tinted with an orange
     accent, Wallpaper, Clear) redraws the dock's icons, each look
     different from every other
  3. in the Wallpaper mode, a red wallpaper then a blue one: the icons
     turn red, then blue
  4. the settings go back to their defaults; the rendered icons are in
     the user's cache (~/.cache/fern/icons)
  5. the machine powers off cleanly

Screenshots and the console log are written to build/image/icons-*.png.
"""

import os
import time

from qemulib import Console, fail, ok, to_png

OUT = "build/image"
LOG = f"{OUT}/qemu-test-icons.log"
MONITOR = f"{OUT}/qemu-test-icons.monitor"
BUS = "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/0/bus"

# 16x16 plain pictures (made with PIL), sent over the console.
RED_PNG = "iVBORw0KGgoAAAANSUhEUgAAABAAAAAQCAIAAACQkWg2AAAAGUlEQVR4nGM8oaHBQApgIkn1qIZRDUNKAwCMLQE4ZidSiAAAAABJRU5ErkJggg=="
BLUE_PNG = "iVBORw0KGgoAAAANSUhEUgAAABAAAAAQCAIAAACQkWg2AAAAGUlEQVR4nGPUcLvEQApgIkn1qIZRDUNKAwAFIgFgt9AkiAAAAABJRU5ErkJggg=="


class Shot:
    def __init__(self, c, name):
        ppm = f"{OUT}/icons-{name}.ppm"
        self.w, self.h, self.px = c.screenshot(ppm)
        to_png(ppm, f"{OUT}/icons-{name}.png")
        os.remove(ppm)

    def pixel(self, x, y):
        i = (y * self.w + x) * 3
        return tuple(self.px[i : i + 3])

    def mean(self, x0, y0, x1, y1):
        pts = [self.pixel(x, y) for y in range(y0, y1, 2) for x in range(x0, x1, 2)]
        n = max(len(pts), 1)
        return tuple(sum(p[c] for p in pts) / n for c in range(3))

    def dock(self):
        """The dock's icons: Downloads and Trash, bottom centre."""
        return self.mean(self.w // 2 - 56, self.h - 62, self.w // 2 + 56, self.h - 18)


def distance(a, b):
    return sum((x - y) ** 2 for x, y in zip(a, b)) ** 0.5


def wait_for(c, name, test, what, timeout=300):
    deadline = time.monotonic() + timeout
    while True:
        shot = Shot(c, name)
        if test(shot):
            return shot
        if time.monotonic() > deadline:
            logs = c.run("fernctl logs -n 300 fern-comp | grep -v 'MESA\\|INFO' | tail -30", "fern-comp's log")
            fail(f"timed out waiting for {what} (last screenshot: {OUT}/icons-{name}.png)\n{logs}")
        c._read()
        time.sleep(1.5)


def setting(c, domain, key, value):
    out = c.run(f"{BUS} fern-settings set {domain} {key} {value}; echo rc=$?", f"{domain}.{key}")
    if "rc=0" not in out:
        fail(f"fern-settings set {domain} {key} {value}: {out}")


def main():
    os.makedirs(OUT, exist_ok=True)
    if os.path.exists(LOG):
        os.remove(LOG)
    c = Console(["image/qemu.sh", "--screen"], LOG, monitor=MONITOR)
    try:
        c.boot_and_login()
        ok("booted and logged in on the serial console")
        c.plain_session()
        deadline = time.monotonic() + 300
        while "shell started" not in c.run("fernctl logs -n 200 fern-comp", "fern-comp's log"):
            if time.monotonic() > deadline:
                fail("fern-comp never started the shell: " + c.run("fernctl logs -n 60 fern-comp"))
            time.sleep(3)
        if not os.path.isdir("de/icons/set/icons"):
            fail("run from the repository root")
        if "folder.downloads.fernicon" not in c.run("ls /usr/share/fern/icons", "the icon set"):
            fail("the icon set is not installed in /usr/share/fern/icons")

        # The dock, with its icons drawn: something blue (the Downloads
        # folder) at the bottom centre.
        wait_for(c, "dock", lambda s: s.dock()[2] > s.dock()[0] + 15, "the dock's icons")
        ok("the dock shows the Downloads and Trash icons (/usr/share/fern/icons)")

        setting(c, "appearance", "accent", "orange")
        seen = []
        for mode in ["default", "dark", "tinted", "wallpaper", "clear"]:
            setting(c, "appearance", "icon_mode", mode)
            # Drawn off the GUI thread, then a 0.45 s crossfade; slow in
            # an emulated VM.
            shot = wait_for(
                c, f"mode-{mode}",
                lambda s: all(distance(s.dock(), before) > 6 for _, before in seen),
                f"the icons in the {mode} mode",
            )
            time.sleep(5)
            shot = Shot(c, f"mode-{mode}")
            now = shot.dock()
            for other, before in seen:
                if distance(now, before) <= 6:
                    fail(f"the dock looks the same in the {mode} and {other} modes: {now} {before}")
            seen.append((mode, now))
            ok(f"icon mode {mode}: the dock's icons redrawn (mean colour {tuple(round(v) for v in now)})")

        # The Wallpaper mode follows the wallpaper.
        setting(c, "appearance", "icon_mode", "wallpaper")
        for name, data in (("red", RED_PNG), ("blue", BLUE_PNG)):
            c.run(f"echo {data} | base64 -d > /root/{name}.png", f"the {name} wallpaper")
        setting(c, "wallpaper", "light", "/root/red.png")
        wait_for(c, "wallpaper-red", lambda s: s.dock()[0] > s.dock()[2] + 10, "red icons")
        setting(c, "wallpaper", "light", "/root/blue.png")
        wait_for(c, "wallpaper-blue", lambda s: s.dock()[2] > s.dock()[0] + 10, "blue icons")
        ok("Wallpaper mode: a red wallpaper turns the icons red, a blue one blue")

        # Back to the defaults, for the other image tests.
        for domain, key in (("appearance", "icon_mode"), ("appearance", "accent"), ("wallpaper", "light")):
            out = c.run(f"{BUS} fern-settings reset {domain} {key}; echo rc=$?", f"resetting {domain}.{key}")
            if "rc=0" not in out:
                fail(f"fern-settings reset {domain} {key}: {out}")

        n = c.run("ls /root/.cache/fern/icons | wc -l", "the icon cache").split()
        if not n or not n[-1].isdigit() or int(n[-1]) < 5:
            fail(f"no rendered icons in ~/.cache/fern/icons: {n}")
        ok(f"{n[-1]} rendered icons in ~/.cache/fern/icons")

        c.send("fernctl poweroff\n")
        deadline = time.monotonic() + 120
        while c.p.poll() is None and time.monotonic() < deadline:
            c._read()
            time.sleep(0.1)
        if c.p.poll() is None:
            fail("the machine did not power off")
        ok("clean poweroff")
        print(f"\nqemu-test-icons: PASS (screenshots in {OUT}/icons-*.png)")
    finally:
        c.close()


if __name__ == "__main__":
    main()
