#!/usr/bin/env python3
"""The compositor on the image's display (`just qemu-test-comp`).

Boots image/qemu.sh --screen (a virtio GPU, no window), logs in on the
serial console and checks, from screenshots taken through the QEMU monitor:
  1. fern-comp takes the display at boot and draws the wallpaper
  2. a striped test window appears with rounded corners, and a transparent
     window that asks for blur shows the stripes behind it blurred
  3. Ctrl+Alt+F2 switches to the console login on tty2 (the session is
     paused) and Ctrl+Alt+F1 brings the desktop back
  4. the machine powers off cleanly

Without host 3D, Mesa renders in software (kms_swrast): slow, but the same
code path as on a GPU. Screenshots and the console log are written to
build/image/.
"""

import os
import time

from qemulib import Console, fail, ok, to_png

OUT = "build/image"
LOG = f"{OUT}/qemu-test-comp.log"
MONITOR = f"{OUT}/qemu-test-comp.monitor"
CLIENT = "/usr/lib/fern/test/fern-testclient"


class Shot:
    def __init__(self, c, name):
        ppm = f"{OUT}/comp-{name}.ppm"
        self.w, self.h, self.px = c.screenshot(ppm)
        to_png(ppm, f"{OUT}/comp-{name}.png")
        os.remove(ppm)

    def pixel(self, x, y):
        i = (y * self.w + x) * 3
        return tuple(self.px[i : i + 3])

    def spread(self, y, x0, x1):
        """Widest channel range along a horizontal line."""
        row = [self.pixel(x, y) for x in range(x0, x1)]
        return max(max(p[c] for p in row) - min(p[c] for p in row) for c in range(3))

    def dark_fraction(self):
        dark = sum(1 for i in range(0, len(self.px), 3 * 97) if max(self.px[i : i + 3]) < 40)
        return dark / (len(self.px) // (3 * 97))


# The test client's stripe colours and their average (fern-testclient).
BANDS = [(0xFF, 0x5A, 0x5F), (0xFF, 0xD1, 0x66), (0x2E, 0xC4, 0xB6), (0x3A, 0x86, 0xFF)]
BLURRED = (153, 157, 158)


def near(p, q, tol):
    return all(abs(a - b) <= tol for a, b in zip(p, q))


def is_wallpaper(s):
    """The grove wallpaper: a green-grey sky at the top, a lighter misty
    band below it, dark forest green at the bottom."""
    top = s.pixel(s.w // 10, s.h // 20)
    mist = s.pixel(s.w // 10, s.h // 4)
    forest = s.pixel(s.w // 10, s.h * 9 // 10)
    return (top[1] > top[0] + 20 and top[1] >= top[2]
            and forest[1] > forest[0] + 20 and max(forest) < 100
            and sum(mist) > sum(forest) + 150)


def has_stripes(s, y, x0, x1):
    """At least three of the four stripe colours along the line."""
    row = [s.pixel(x, y) for x in range(x0, x1)]
    return sum(any(near(p, b, 12) for p in row) for b in BANDS) >= 3


def is_blurred(s, y, x0, x1):
    return s.spread(y, x0, x1) < 60 and near(s.pixel((x0 + x1) // 2, y), BLURRED, 20)


def wait_for(c, name, test, what, timeout=300):
    """Takes screenshots until `test(shot)` holds."""
    deadline = time.monotonic() + timeout
    while True:
        shot = Shot(c, name)
        if test(shot):
            return shot
        if time.monotonic() > deadline:
            fail(f"timed out waiting for {what} (last screenshot: {OUT}/comp-{name}.png)")
        c._read()
        time.sleep(2)


def start_client(c, args):
    """Starts the test client in the background (in a subshell: run()
    appends "; echo", which cannot follow a bare "&")."""
    c.run(f"({CLIENT} {args} >/dev/null 2>&1 &)", f"starting fern-testclient {args}")


def placed(w, h, size):
    """Where fern-comp's placement puts a new window: centred, 40% down."""
    sw, sh = size
    return (sw - w) // 2, int((sh - h) * 0.4)


def main():
    os.makedirs(OUT, exist_ok=True)
    if os.path.exists(LOG):
        os.remove(LOG)
    c = Console(["image/qemu.sh", "--screen"], LOG, monitor=MONITOR)
    try:
        c.boot_and_login()
        ok("booted and logged in on the serial console")

        deadline = time.monotonic() + 300
        while "display ready" not in c.run("fernctl logs -n 200 fern-comp", "fern-comp's log"):
            if time.monotonic() > deadline:
                fail("fern-comp never reported a display: " + c.run("fernctl status fern-comp"))
            time.sleep(3)
        ok("fern-comp took the display")

        shot = wait_for(c, "wallpaper", is_wallpaper, "the wallpaper")
        size = (shot.w, shot.h)
        ok(f"the wallpaper is on screen ({shot.w}x{shot.h})")

        sock = c.run("ls /run/user/0 | grep -m1 '^wayland-[0-9]*$'", "the Wayland socket")
        if not sock.startswith("wayland-"):
            fail(f"no Wayland socket in /run/user/0: {sock!r}")
        c.run(f"export XDG_RUNTIME_DIR=/run/user/0 WAYLAND_DISPLAY={sock}", "the client environment")
        globals = c.run(f"{CLIENT} --globals", "the compositor's globals")
        for g in ("zwp_linux_dmabuf_v1", "org_kde_kwin_blur_manager", "xdg_wm_base"):
            if g not in globals:
                fail(f"fern-comp does not offer {g}: {globals!r}")
        sync = "with" if "wp_linux_drm_syncobj_manager_v1" in globals else "without"
        ok(f"protocols: GPU buffers (linux-dmabuf), blur, xdg-shell; {sync} explicit sync on this GPU")
        start_client(c, "--stripes --size 600x400")
        sx, sy = placed(600, 400, size)
        wait_for(c, "stripes", lambda s: has_stripes(s, sy + 200, sx + 50, sx + 550), "the striped window")
        ok("a client window is drawn")

        start_client(c, "--color ffffff00 --size 400x200 --blur")
        gx, gy = placed(400, 200, size)
        shot = wait_for(
            c, "glass",
            lambda s: is_blurred(s, gy + 100, gx + 60, gx + 340) and has_stripes(s, sy + 30, sx + 50, sx + 550),
            "the blurred glass window",
        )
        ok("blur: the stripes behind the glass are smoothed, the rest stay sharp")
        # A corner of the striped window is cut: its corner pixel shows the
        # wallpaper, a pixel one radius in along the edge shows a stripe.
        corner, edge = shot.pixel(sx, sy), shot.pixel(sx + 14, sy)
        if any(near(corner, b, 40) for b in BANDS) or not any(near(edge, b, 12) for b in BANDS):
            fail(f"window corner not rounded: corner {corner}, edge {edge}")
        ok("rounded window corners")

        c.key("ctrl-alt-f2")
        wait_for(c, "tty2", lambda s: s.dark_fraction() > 0.9, "the console on tty2", 120)
        ok("Ctrl+Alt+F2: the console on tty2 (desktop paused)")
        c.key("ctrl-alt-f1")
        wait_for(
            c, "back",
            lambda s: is_blurred(s, gy + 100, gx + 60, gx + 340) and has_stripes(s, sy + 30, sx + 50, sx + 550),
            "the desktop after switching back",
            180,
        )
        ok("Ctrl+Alt+F1: the desktop is back")

        c.send("fernctl poweroff\n")
        deadline = time.monotonic() + 120
        while c.p.poll() is None and time.monotonic() < deadline:
            c._read()
            time.sleep(0.1)
        if c.p.poll() is None:
            fail("the machine did not power off")
        ok("clean poweroff")
        print(f"\nqemu-test-comp: PASS (screenshots in {OUT}/comp-*.png)")
    finally:
        c.close()


if __name__ == "__main__":
    main()
