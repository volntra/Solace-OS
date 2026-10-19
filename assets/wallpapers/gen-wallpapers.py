#!/usr/bin/env python3
"""Generates Solace's wallpapers as SVG (original artwork).

  dusk.svg          the default wallpaper: a still dusk sea under a warm
                    horizon glow, layered soft swells in the foreground
  dusk-blurred.svg  the same, heavily blurred and slightly dimmed, for the
                    boot menu, login screen, and installer backgrounds

Rendered to PNG by `just wallpapers` (resvg). Palette matches the logo
(docs/design/HIG.md, "Brand").
"""

import math
import pathlib
import random

OUT = pathlib.Path(__file__).resolve().parent
W, H = 2560, 1440
HORIZON = 0.60 * H


def swell(y0, amp, wl, phase, seed):
    """A smooth closed path: a gently undulating band from y0 to the bottom."""
    rnd = random.Random(seed)
    pts = []
    steps = 48
    for i in range(steps + 1):
        x = W * i / steps
        y = (y0 + amp * math.sin(2 * math.pi * x / wl + phase)
             + amp * 0.35 * math.sin(2 * math.pi * x / (wl * 0.43) + phase * 1.7)
             + rnd.uniform(-amp * 0.04, amp * 0.04))
        pts.append((x, y))
    # Catmull-Rom to cubic Bezier for smoothness.
    d = f"M{pts[0][0]:.1f} {pts[0][1]:.1f}"
    for i in range(len(pts) - 1):
        p0 = pts[max(i - 1, 0)]
        p1, p2 = pts[i], pts[i + 1]
        p3 = pts[min(i + 2, len(pts) - 1)]
        c1 = (p1[0] + (p2[0] - p0[0]) / 6, p1[1] + (p2[1] - p0[1]) / 6)
        c2 = (p2[0] - (p3[0] - p1[0]) / 6, p2[1] - (p3[1] - p1[1]) / 6)
        d += f" C{c1[0]:.1f} {c1[1]:.1f} {c2[0]:.1f} {c2[1]:.1f} {p2[0]:.1f} {p2[1]:.1f}"
    d += f" L{W} {H + 10} L0 {H + 10} Z"
    return d


def scene(blurred):
    sun_x, sun_y = W * 0.62, HORIZON
    layers = [
        # (y0, amplitude, wavelength, phase, colour top, colour bottom)
        (HORIZON + 70, 14, 1900, 0.6, "#40357F", "#2A2565"),
        (HORIZON + 170, 26, 1500, 2.1, "#2F2A6D", "#1E1B50"),
        (HORIZON + 300, 40, 1300, 4.0, "#221F58", "#151441"),
        (HORIZON + 450, 55, 1700, 1.2, "#181645", "#0E0D2E"),
    ]
    defs = [f"""
    <linearGradient id="sky" x1="0" y1="0" x2="0" y2="{HORIZON}" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#121444"/>
      <stop offset="0.38" stop-color="#2E2A78"/>
      <stop offset="0.72" stop-color="#8E4E8E"/>
      <stop offset="0.92" stop-color="#E88C78"/>
      <stop offset="1" stop-color="#F9B184"/>
    </linearGradient>
    <linearGradient id="sea" x1="0" y1="{HORIZON}" x2="0" y2="{H}" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#5A4590"/>
      <stop offset="0.25" stop-color="#2C286B"/>
      <stop offset="1" stop-color="#0D0C2C"/>
    </linearGradient>
    <radialGradient id="glow" cx="{sun_x}" cy="{sun_y}" r="{W * 0.42}" gradientUnits="userSpaceOnUse"
                    gradientTransform="translate({sun_x} {sun_y}) scale(1 0.55) translate({-sun_x} {-sun_y})">
      <stop offset="0" stop-color="#FFE0A8" stop-opacity="0.95"/>
      <stop offset="0.18" stop-color="#FFB38A" stop-opacity="0.55"/>
      <stop offset="0.5" stop-color="#E77F8F" stop-opacity="0.18"/>
      <stop offset="1" stop-color="#E77F8F" stop-opacity="0"/>
    </radialGradient>
    <radialGradient id="path" cx="{sun_x}" cy="{HORIZON}" r="{H - HORIZON}" gradientUnits="userSpaceOnUse"
                    gradientTransform="translate({sun_x} {HORIZON}) scale(0.55 1) translate({-sun_x} {-HORIZON})">
      <stop offset="0" stop-color="#FFE2AE" stop-opacity="0.55"/>
      <stop offset="0.3" stop-color="#FFB78C" stop-opacity="0.2"/>
      <stop offset="1" stop-color="#FFB78C" stop-opacity="0"/>
    </radialGradient>
    <filter id="grain" x="0" y="0" width="100%" height="100%">
      <feTurbulence type="fractalNoise" baseFrequency="0.9" numOctaves="2" seed="7" result="n"/>
      <feColorMatrix in="n" type="matrix"
        values="0 0 0 0 1  0 0 0 0 1  0 0 0 0 1  0 0 0 0.035 0"/>
    </filter>
    <filter id="soft"><feGaussianBlur stdDeviation="6"/></filter>"""]
    for i, (_, _, _, _, top, bottom) in enumerate(layers):
        defs.append(f"""
    <linearGradient id="l{i}" x1="0" y1="{HORIZON}" x2="0" y2="{H}" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="{top}"/><stop offset="1" stop-color="{bottom}"/>
    </linearGradient>""")
    if blurred:
        defs.append("""
    <filter id="blur" x="-5%" y="-5%" width="110%" height="110%">
      <feGaussianBlur stdDeviation="48"/>
    </filter>""")

    body = [f'<rect width="{W}" height="{HORIZON + 2}" fill="url(#sky)"/>',
            f'<rect y="{HORIZON}" width="{W}" height="{H - HORIZON}" fill="url(#sea)"/>',
            f'<rect width="{W}" height="{H}" fill="url(#glow)"/>']
    # The sun's light path on the water: a soft tapering column.
    body.append(f'<rect y="{HORIZON}" width="{W}" height="{H - HORIZON}" fill="url(#path)"/>')
    for i, (y0, amp, wl, ph, _, _) in enumerate(layers):
        body.append(f'<path d="{swell(y0, amp, wl, ph, i)}" fill="url(#l{i})" opacity="0.9"/>')
        # Each swell catches a little of the light on its crest.
        body.append(f'<path d="{swell(y0, amp, wl, ph, i)}" fill="url(#path)" opacity="{0.55 - i * 0.12:.2f}"/>')
    # Horizon haze line.
    body.append(f'<rect y="{HORIZON - 3}" width="{W}" height="6" fill="#FFE4C0" opacity="0.35" filter="url(#soft)"/>')
    body.append(f'<rect width="{W}" height="{H}" filter="url(#grain)"/>')

    inner = "\n  ".join(body)
    if blurred:
        # Scale the scene up a little so the blur's edge falloff (it samples
        # transparency outside the canvas) lands off-screen.
        inner = (f'<g filter="url(#blur)"><g transform="translate({W / 2} {H / 2}) scale(1.12) '
                 f'translate({-W / 2} {-H / 2})">\n  {inner}\n  </g></g>\n'
                 f'  <rect width="{W}" height="{H}" fill="#0A0A24" opacity="0.28"/>')
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}">
  <title>Dusk</title>
  <!-- Original artwork for Solace. Generated by assets/wallpapers/gen-wallpapers.py. -->
  <defs>{''.join(defs)}
  </defs>
  {inner}
</svg>
"""


def main():
    (OUT / "dusk.svg").write_text(scene(False))
    (OUT / "dusk-blurred.svg").write_text(scene(True))
    print(f"wrote {OUT}/dusk.svg and dusk-blurred.svg")


if __name__ == "__main__":
    main()
