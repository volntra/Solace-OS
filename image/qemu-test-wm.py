#!/usr/bin/env python3
"""The window manager on the image (`just qemu-test-wm`), the Phase 5 demo.

Boots image/qemu.sh --screen, logs in on the serial console, opens two
test windows and drives fern-wm with real key presses through the QEMU
monitor, checking screenshots:
  1. fern-comp started fern-wm; new windows get titlebars and do not
     overlap
  2. Super+Left / Super+Right tile the focused window to a half
  3. Ctrl+Super+Shift+Right carries it to a new Space (the other window
     slides away); Ctrl+Super+Left comes back
  4. Ctrl+Super+Up shows Overview (dimmed desktop, a Spaces strip with two
     Spaces); Escape leaves it
  5. killing fern-wm: every window stays where it is, fern-comp starts a
     new fern-wm, and it knows the desktop (tiling works again)
  6. the machine powers off cleanly

Screenshots and the console log are written to build/image/wm-*.png.
"""

import os
import time

from qemulib import Console, fail, ok, to_png

OUT = "build/image"
LOG = f"{OUT}/qemu-test-wm.log"
MONITOR = f"{OUT}/qemu-test-wm.monitor"
CLIENT = "/usr/lib/fern/test/fern-testclient"
TITLEBAR = 28
GAP = 8
# The shell keeps the menu bar's height at the top and the dock's room at
# the bottom out of the work area (generous: the dock's size varies).
MENUBAR = 28
DOCK_ROOM = 110
RED = (0xFF, 0x5A, 0x5F)
BLUE = (0x3A, 0x86, 0xFF)
LEAF = (0x7C, 0xC4, 0x8A)
TITLEBAR_COLOURS = [(0xEA, 0xEC, 0xE7), (0xF3, 0xF4, 0xF1)]


class Shot:
    def __init__(self, c, name):
        ppm = f"{OUT}/wm-{name}.ppm"
        self.name = name
        self.w, self.h, self.px = c.screenshot(ppm)
        to_png(ppm, f"{OUT}/wm-{name}.png")
        os.remove(ppm)

    def pixel(self, x, y):
        i = (y * self.w + x) * 3
        return tuple(self.px[i : i + 3])

    def fraction(self, colour, tol=10, x0=0, y0=0, x1=None, y1=None):
        """How much of a region (sampled) is `colour`."""
        x1, y1 = x1 or self.w, y1 or self.h
        pts = [(x, y) for y in range(y0, y1, 9) for x in range(x0, x1, 9)]
        return sum(1 for x, y in pts if near(self.pixel(x, y), colour, tol)) / max(len(pts), 1)


def near(p, q, tol):
    return all(abs(a - b) <= tol for a, b in zip(p, q))


def wait_for(c, name, test, what, timeout=240):
    deadline = time.monotonic() + timeout
    while True:
        shot = Shot(c, name)
        if test(shot):
            return shot
        if time.monotonic() > deadline:
            logs = c.run("fernctl logs -n 30 fern-comp", "fern-comp's log")
            fail(f"timed out waiting for {what} (last screenshot: {OUT}/wm-{name}.png)\n{logs}")
        c._read()
        time.sleep(2)


def start_client(c, args):
    c.run(f"({CLIENT} {args} >/dev/null 2>&1 &)", f"starting fern-testclient {args}")


def left_half(s, colour):
    return s.fraction(colour, x0=GAP + 10, y0=MENUBAR + GAP + TITLEBAR + 10, x1=s.w // 2 - 10, y1=s.h - DOCK_ROOM) > 0.95


def right_half(s, colour):
    return s.fraction(colour, x0=s.w // 2 + 10, y0=MENUBAR + GAP + TITLEBAR + 10, x1=s.w - GAP - 10, y1=s.h - DOCK_ROOM) > 0.95


# The image has no procps: processes are found in /proc.
FIND_WM = 'for p in /proc/[0-9]*; do [ "$(cat $p/comm 2>/dev/null)" = fern-wm ] && echo ${p#/proc/}; done; echo end'


def wm_pid(c):
    out = c.run(FIND_WM, "fern-wm's pid").split()
    pids = [w for w in out if w.isdigit()]
    return pids[0] if pids else "none"


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
        while "display ready" not in c.run("fernctl logs -n 200 fern-comp", "fern-comp's log"):
            if time.monotonic() > deadline:
                fail("fern-comp never reported a display: " + c.run("fernctl status fern-comp"))
            time.sleep(3)
        pid = wm_pid(c)
        if not pid.isdigit():
            fail("fern-comp did not start fern-wm: " + c.run("fernctl logs -n 50 fern-comp"))
        ok(f"fern-comp took the display and started fern-wm (pid {pid})")

        sock = c.run("ls /run/user/0 | grep -m1 '^wayland-[0-9]*$'", "the Wayland socket")
        c.run(f"export XDG_RUNTIME_DIR=/run/user/0 WAYLAND_DISPLAY={sock}", "the client environment")
        start_client(c, "--color ff5a5fff --size 420x300 --title Red --app-id org.fern.Red")
        wait_for(c, "red", lambda s: s.fraction(RED) > 0.05, "the red window")
        start_client(c, "--color 3a86ffff --size 420x300 --title Blue --app-id org.fern.Blue")
        shot = wait_for(c, "two", lambda s: s.fraction(RED) > 0.1 and s.fraction(BLUE) > 0.1, "two windows")
        bars = shot.fraction(TITLEBAR_COLOURS[0], 8) + shot.fraction(TITLEBAR_COLOURS[1], 8)
        if bars < 0.01:
            fail("no titlebars on screen")
        # Equal areas, each fully shown: they do not overlap.
        if abs(shot.fraction(RED) - shot.fraction(BLUE)) > 0.02:
            fail(f"windows overlap: red {shot.fraction(RED):.3f}, blue {shot.fraction(BLUE):.3f}")
        ok("two windows with titlebars, placed side by side")

        # Blue (newest) has focus.
        c.key("meta_l-left")
        wait_for(c, "tile-left", lambda s: left_half(s, BLUE), "Blue on the left half (Super+Left)")
        ok("Super+Left: Blue fills the left half")
        c.key("meta_l-right")
        wait_for(c, "tile-right", lambda s: right_half(s, BLUE), "Blue on the right half (Super+Right)")
        ok("Super+Right: Blue fills the right half")

        c.key("ctrl-meta_l-shift-right")
        wait_for(
            c, "space-2",
            lambda s: right_half(s, BLUE) and s.fraction(RED) < 0.001,
            "Blue alone on a new Space",
        )
        ok("Ctrl+Super+Shift+Right: Blue carried to a new Space, Red slid away")
        c.key("ctrl-meta_l-left")
        wait_for(
            c, "space-1",
            lambda s: s.fraction(RED) > 0.1 and s.fraction(BLUE) < 0.001,
            "Red back on the first Space",
        )
        ok("Ctrl+Super+Left: back to the first Space")

        c.key("ctrl-meta_l-up")
        shot = wait_for(
            c, "overview",
            lambda s: s.fraction(LEAF, 16, y1=s.h // 4) > 0.002 and s.fraction(RED) > 0.02,
            "Overview with the Spaces strip",
        )
        ok("Ctrl+Super+Up: Overview, with the Spaces strip")
        c.key("esc")
        wait_for(
            c, "after-overview",
            lambda s: s.fraction(LEAF, 16, y1=s.h // 4) < 0.0005 and s.fraction(RED) > 0.1,
            "the desktop after Escape",
        )
        ok("Escape: back from Overview")

        before = Shot(c, "before-kill")
        red_before = before.fraction(RED)
        c.run(f"kill {pid}", "killing fern-wm")
        time.sleep(0.1)
        during = Shot(c, "during-restart")
        if abs(during.fraction(RED) - red_before) > 0.005:
            fail("windows changed when fern-wm died")
        # Software emulation: a restart (and the shell's re-layout) can take
        # minutes there.
        deadline = time.monotonic() + 300
        new = wm_pid(c)
        while not new.isdigit() or new == pid:
            if time.monotonic() > deadline:
                fail("fern-comp did not restart fern-wm")
            time.sleep(1)
            new = wm_pid(c)
        ok(f"fern-wm killed: the windows stayed, a new fern-wm took over (pid {new})")
        # The new window manager knows the desktop: Red has focus and tiles.
        c.key("meta_l-left")
        wait_for(c, "after-restart", lambda s: left_half(s, RED), "Red tiling after the restart")
        ok("the new fern-wm works (Super+Left tiles Red)")
        c.key("ctrl-meta_l-right")
        wait_for(c, "space-2-again", lambda s: right_half(s, BLUE), "Blue's Space after the restart")
        ok("it remembers the Spaces: Blue is still on the second one")

        c.send("fernctl poweroff\n")
        deadline = time.monotonic() + 120
        while c.p.poll() is None and time.monotonic() < deadline:
            c._read()
            time.sleep(0.1)
        if c.p.poll() is None:
            fail("the machine did not power off")
        ok("clean poweroff")
        print(f"\nqemu-test-wm: PASS (screenshots in {OUT}/wm-*.png)")
    finally:
        c.close()


if __name__ == "__main__":
    main()
