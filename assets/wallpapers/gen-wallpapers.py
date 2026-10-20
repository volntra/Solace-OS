#!/usr/bin/env python3
"""Generates Fern's wallpapers as SVG (original artwork).

  grove.svg          the default wallpaper: ridges of pine forest fading
                     into morning mist, under a soft pale sky
  grove-blurred.svg  the same, heavily blurred and slightly dimmed, for the
                     boot menu, login screen, and installer backgrounds

Rendered to PNG by `just wallpapers` (resvg). Palette matches the logo
(docs/design/HIG.md, "Brand").
"""

import math
import pathlib
import random

OUT = pathlib.Path(__file__).resolve().parent
W, H = 2560, 1440

# Ridges from far to near: (base height as a fraction of H, hill amplitude,
# wavelength, phase, tree height range, colour). Far ridges are pale and
# misty, near ones deep forest green.
RIDGES = [
    (0.50, 46, 2100, 0.4, (26, 44), "#A9C2AF"),
    (0.58, 60, 1700, 2.2, (34, 58), "#7FA38A"),
    (0.67, 70, 1500, 4.1, (44, 74), "#55806A"),
    (0.77, 80, 1900, 1.3, (58, 96), "#30594A"),
    (0.89, 90, 1600, 3.3, (76, 128), "#173D2E"),
]


def ridge_y(x, base, amp, wl, phase):
    return (base + amp * math.sin(2 * math.pi * x / wl + phase)
            + amp * 0.4 * math.sin(2 * math.pi * x / (wl * 0.37) + phase * 1.9))


def ridge(i, base_f, amp, wl, phase, heights):
    """One ridge: a smooth hill line with a row of pines standing on it,
    closed down to the bottom of the screen."""
    rnd = random.Random(100 + i)
    base = base_f * H
    # The hill itself.
    steps = 64
    d = ""
    for s in range(steps + 1):
        x = W * s / steps
        d += f"{'M' if s == 0 else 'L'}{x:.1f} {ridge_y(x, base, amp, wl, phase):.1f} "
    d += f"L{W} {H + 10} L0 {H + 10} Z"
    # Pines along it, closely packed with gaps here and there.
    trees = []
    x = -40.0
    while x < W + 40:
        h = rnd.uniform(*heights)
        w = h * rnd.uniform(0.34, 0.42)
        y = ridge_y(x, base, amp, wl, phase) + h * 0.12
        # A cone with two shallow branch steps, like the logo.
        t1, t2 = y - h * 0.62, y - h * 0.32
        trees.append(
            f"M{x:.1f} {y - h:.1f}"
            f"L{x + w * 0.26:.1f} {t1:.1f}L{x + w * 0.16:.1f} {t1 + h * 0.03:.1f}"
            f"L{x + w * 0.38:.1f} {t2:.1f}L{x + w * 0.26:.1f} {t2 + h * 0.03:.1f}"
            f"L{x + w * 0.5:.1f} {y:.1f}L{x - w * 0.5:.1f} {y:.1f}"
            f"L{x - w * 0.26:.1f} {t2 + h * 0.03:.1f}L{x - w * 0.38:.1f} {t2:.1f}"
            f"L{x - w * 0.16:.1f} {t1 + h * 0.03:.1f}L{x - w * 0.26:.1f} {t1:.1f}Z")
        x += w * rnd.uniform(0.45, 0.8)
        if rnd.random() < 0.06:
            x += w * rnd.uniform(1.5, 4)
    return d, "".join(trees)


def scene(blurred):
    defs = [f"""
    <linearGradient id="sky" x1="0" y1="0" x2="0" y2="{H * 0.62}" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#2E4F43"/>
      <stop offset="0.45" stop-color="#7C9C8C"/>
      <stop offset="0.85" stop-color="#D3E2D3"/>
      <stop offset="1" stop-color="#E9F0E4"/>
    </linearGradient>
    <radialGradient id="sun" cx="{W * 0.68}" cy="{H * 0.36}" r="{W * 0.36}" gradientUnits="userSpaceOnUse"
                    gradientTransform="translate({W * 0.68} {H * 0.36}) scale(1 0.7) translate({-W * 0.68} {-H * 0.36})">
      <stop offset="0" stop-color="#FFF8E4" stop-opacity="0.85"/>
      <stop offset="0.25" stop-color="#F4F2DA" stop-opacity="0.4"/>
      <stop offset="1" stop-color="#F4F2DA" stop-opacity="0"/>
    </radialGradient>
    <filter id="grain" x="0" y="0" width="100%" height="100%">
      <feTurbulence type="fractalNoise" baseFrequency="0.9" numOctaves="2" seed="7" result="n"/>
      <feColorMatrix in="n" type="matrix"
        values="0 0 0 0 1  0 0 0 0 1  0 0 0 0 1  0 0 0 0.035 0"/>
    </filter>"""]
    # Mist between ridges: a band of pale haze rising from each ridge's foot.
    for i, (base_f, *_rest) in enumerate(RIDGES[:-1]):
        y = base_f * H
        defs.append(f"""
    <linearGradient id="mist{i}" x1="0" y1="{y - 40}" x2="0" y2="{y + 150}" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#E4EDE2" stop-opacity="0"/>
      <stop offset="0.6" stop-color="#E4EDE2" stop-opacity="{0.55 - i * 0.1:.2f}"/>
      <stop offset="1" stop-color="#E4EDE2" stop-opacity="0"/>
    </linearGradient>""")
    if blurred:
        defs.append("""
    <filter id="blur" x="-5%" y="-5%" width="110%" height="110%">
      <feGaussianBlur stdDeviation="48"/>
    </filter>""")

    body = [f'<rect width="{W}" height="{H}" fill="url(#sky)"/>',
            f'<rect width="{W}" height="{H}" fill="url(#sun)"/>']
    for i, (base_f, amp, wl, ph, heights, colour) in enumerate(RIDGES):
        hill, trees = ridge(i, base_f, amp, wl, ph, heights)
        body.append(f'<path d="{hill}" fill="{colour}"/>')
        body.append(f'<path d="{trees}" fill="{colour}"/>')
        if i < len(RIDGES) - 1:
            y = base_f * H
            body.append(f'<rect y="{y - 40:.0f}" width="{W}" height="190" fill="url(#mist{i})"/>')
    body.append(f'<rect width="{W}" height="{H}" filter="url(#grain)"/>')

    inner = "\n  ".join(body)
    if blurred:
        # Scale the scene up a little so the blur's edge falloff (it samples
        # transparency outside the canvas) lands off-screen.
        inner = (f'<g filter="url(#blur)"><g transform="translate({W / 2} {H / 2}) scale(1.12) '
                 f'translate({-W / 2} {-H / 2})">\n  {inner}\n  </g></g>\n'
                 f'  <rect width="{W}" height="{H}" fill="#0B1A13" opacity="0.42"/>')
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}">
  <title>Grove</title>
  <!-- Original artwork for Fern. Generated by assets/wallpapers/gen-wallpapers.py. -->
  <defs>{''.join(defs)}
  </defs>
  {inner}
</svg>
"""


def main():
    (OUT / "grove.svg").write_text(scene(False))
    (OUT / "grove-blurred.svg").write_text(scene(True))
    print(f"wrote {OUT}/grove.svg and grove-blurred.svg")


if __name__ == "__main__":
    main()
