#!/usr/bin/env python3
"""Generates Fern's wallpapers as SVG (original artwork).

  grove.svg          the default wallpaper: a spruce forest at dawn, ridge
                     behind ridge fading into fog, light falling through
                     the mist, a near tree out of focus at the edge
  grove-night.svg    the same forest at night, the dark-mode partner of Grove
                     (a moon in the haze, faint stars, cool fog)
  grove-blurred.svg  the same, heavily blurred and slightly dimmed, for the
                     boot menu, login screen, and installer backgrounds

It aims for a photograph rather than a flat drawing: every tree is a
ragged spruce silhouette (tiers of drooping branches, no two alike), each
ridge is softened by how far away it is, the fog is textured noise rather
than a band, and the frame has light rays, grain and a vignette. Rendered
to PNG by `just wallpapers` (resvg). Palette matches the logo
(docs/design/HIG.md, "Brand").
"""

import math
import pathlib
import random

OUT = pathlib.Path(__file__).resolve().parent
W, H = 2560, 1440
SUN = (W * 0.70, H * 0.17)
FOG = "#D7E2D6"

# Ridges from far to near: (baseline as a fraction of H, terrain
# amplitude, tree height range, spacing factor, colour, blur, fog above).
RIDGES = [
    (0.47, 20, (22, 40), 0.55, "#A9BDAE", 1.4, 0.55),
    (0.53, 28, (30, 56), 0.55, "#8BA594", 1.1, 0.50),
    (0.60, 36, (44, 80), 0.6, "#68877A", 0.8, 0.45),
    (0.68, 44, (64, 118), 0.6, "#46685A", 0.5, 0.40),
    (0.78, 52, (96, 170), 0.65, "#2A4A3D", 0.25, 0.30),
    (0.92, 60, (150, 260), 0.65, "#162E24", 0.0, 0.0),
]


def f(v):
    return f"{v:.1f}".rstrip("0").rstrip(".")


def terrain(x, base, amp, seed):
    return (base + amp * math.sin(x / 410 + seed)
            + amp * 0.5 * math.sin(x / 157 + seed * 2.3)
            + amp * 0.25 * math.sin(x / 61 + seed * 0.7))


def spruce(rnd, x, y, h):
    """A spruce silhouette standing at (x, y), h tall: a narrow spire on
    top, then tiers of drooping branches whose tips grow longer towards
    the foot, each side ragged on its own."""
    width = h * rnd.uniform(0.30, 0.40)
    tiers = max(6, int(h / 7))
    lean = rnd.uniform(-0.03, 0.03) * h

    def side(sign):
        pts = []
        for i in range(1, tiers + 1):
            t = i / tiers                     # 0 at the tip, 1 at the foot
            yy = y - h + h * t
            half = width / 2 * (0.15 + 0.85 * t ** 0.9) * rnd.uniform(0.75, 1.15)
            cx = x + lean * (1 - t)
            gap = h / tiers
            # Branch tip, drooping a little; then back in towards the trunk.
            pts.append((cx + sign * half, yy + gap * rnd.uniform(0.15, 0.45)))
            if rnd.random() < 0.5:
                pts.append((cx + sign * half * rnd.uniform(0.7, 0.9), yy + gap * 0.55))
            pts.append((cx + sign * half * rnd.uniform(0.30, 0.55), yy + gap * rnd.uniform(0.55, 0.8)))
        return pts

    tip = (x + lean, y - h - h * 0.04)
    right = side(1)
    left = side(-1)
    trunk = h * 0.05
    pts = [tip] + right + [(x + trunk, y), (x - trunk, y)] + list(reversed(left))
    return "M" + "L".join(f"{f(px)} {f(py)}" for px, py in pts) + "Z", width


def ridge(i, base_f, amp, heights, spacing):
    rnd = random.Random(1000 + i)
    base = base_f * H
    steps = 80
    # The forest is a mass: the ridge's ground sits part way up the trees,
    # so only their upper halves stand clear against what is behind.
    lift = sum(heights) / 2 * 0.42
    ground = "M" + "L".join(
        f"{f(W * s / steps)} {f(terrain(W * s / steps, base, amp, i * 1.7) - lift)}"
        for s in range(steps + 1))
    ground += f"L{W} {H + 10}L0 {H + 10}Z"
    trees = []
    x = -60.0
    while x < W + 60:
        h = rnd.uniform(*heights) * rnd.choice((0.55, 0.75, 1.0, 1.0, 1.0, 1.15))
        y = terrain(x, base, amp, i * 1.7) - lift + h * rnd.uniform(0.3, 0.5)
        path, width = spruce(rnd, x, y, h)
        trees.append(path)
        x += width * spacing * rnd.uniform(0.2, 0.65)
        if rnd.random() < 0.025:
            x += h * rnd.uniform(0.6, 1.8)   # a clearing
    return ground, "".join(trees)


def foreground_tree():
    """A big spruce close to the camera on the left edge, far out of focus."""
    rnd = random.Random(7)
    return spruce(rnd, W * 0.035, H * 1.08, H * 1.25)[0]


def night(colour):
    """A ridge colour at night: darker, cooler."""
    r, g, b = (int(colour[i:i + 2], 16) for i in (1, 3, 5))
    t = (0x0A, 0x16, 0x1C)
    mix = [round(c * 0.38 + d * 0.62) for c, d in zip((r, g, b), t)]
    return "#" + "".join(f"{c:02X}" for c in mix)


def scene(blurred, at_night=False):
    sky = (["#070E12", "#0F1D22", "#1C2F33", "#2A3E3E"] if at_night
           else ["#3D5A4E", "#7E998B", "#C6D5C6", "#E4EBDD"])
    glow = ([("#DCE6F0", 0.55), ("#B9C9D6", 0.22), ("#8FA3B0", 0.07), ("#8FA3B0", 0)] if at_night
            else [("#FFF4D8", 0.8), ("#F8EFD4", 0.5), ("#EEF0DA", 0.2), ("#EEF0DA", 0)])
    glow_r = W * (0.22 if at_night else 0.42)
    fog_rgb = "0.36 0.43 0.46" if at_night else "0.85 0.89 0.84"
    fog_colour = "#4C5E62" if at_night else FOG
    defs = [f"""
    <linearGradient id="sky" x1="0" y1="0" x2="0" y2="{H * 0.6}" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="{sky[0]}"/>
      <stop offset="0.35" stop-color="{sky[1]}"/>
      <stop offset="0.75" stop-color="{sky[2]}"/>
      <stop offset="1" stop-color="{sky[3]}"/>
    </linearGradient>
    <radialGradient id="sun" cx="{SUN[0]}" cy="{SUN[1]}" r="{glow_r}" gradientUnits="userSpaceOnUse"
                    gradientTransform="translate({SUN[0]} {SUN[1]}) scale(1 0.8) translate({-SUN[0]} {-SUN[1]})">
      <stop offset="0" stop-color="{glow[0][0]}" stop-opacity="{glow[0][1]}"/>
      <stop offset="0.12" stop-color="{glow[1][0]}" stop-opacity="{glow[1][1]}"/>
      <stop offset="0.4" stop-color="{glow[2][0]}" stop-opacity="{glow[2][1]}"/>
      <stop offset="1" stop-color="{glow[3][0]}" stop-opacity="{glow[3][1]}"/>
    </radialGradient>
    <radialGradient id="vignette" cx="{W / 2}" cy="{H * 0.45}" r="{W * 0.62}" gradientUnits="userSpaceOnUse">
      <stop offset="0.55" stop-color="#06100B" stop-opacity="0"/>
      <stop offset="1" stop-color="#06100B" stop-opacity="0.55"/>
    </radialGradient>
    <filter id="grain" x="0" y="0" width="100%" height="100%">
      <feTurbulence type="fractalNoise" baseFrequency="0.85" numOctaves="2" seed="11" result="n"/>
      <feColorMatrix in="n" type="matrix"
        values="0 0 0 0 1  0 0 0 0 1  0 0 0 0 1  0 0 0 0.05 0"/>
    </filter>
    <filter id="rays" x="-20%" y="-20%" width="140%" height="140%">
      <feGaussianBlur stdDeviation="26"/>
    </filter>
    <filter id="bokeh" x="-30%" y="-10%" width="160%" height="120%">
      <feGaussianBlur stdDeviation="22"/>
    </filter>"""]
    # Textured fog: low-frequency noise stretched sideways, tinted pale.
    for i, (*_, fog) in enumerate(RIDGES):
        if fog <= 0:
            continue
        defs.append(f"""
    <filter id="fog{i}" x="0" y="0" width="100%" height="100%">
      <feTurbulence type="fractalNoise" baseFrequency="0.0013 0.0065" numOctaves="4" seed="{20 + i}" result="n"/>
      <feColorMatrix in="n" type="matrix"
        values="0 0 0 0 {fog_rgb.split()[0]}  0 0 0 0 {fog_rgb.split()[1]}  0 0 0 0 {fog_rgb.split()[2]}  0 0 0 1.9 -0.55"/>
    </filter>
    <linearGradient id="fogband{i}" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#FFFFFF" stop-opacity="0"/>
      <stop offset="0.55" stop-color="#FFFFFF" stop-opacity="{fog:.2f}"/>
      <stop offset="1" stop-color="#FFFFFF" stop-opacity="0"/>
    </linearGradient>
    <mask id="fogmask{i}" maskContentUnits="userSpaceOnUse">
      <rect y="{RIDGES[i][0] * H - 150:.0f}" width="{W}" height="260" fill="url(#fogband{i})"/>
    </mask>
    <linearGradient id="haze{i}" x1="0" y1="{RIDGES[i][0] * H - 140:.0f}" x2="0" y2="{RIDGES[i][0] * H + 90:.0f}"
                    gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="{fog_colour}" stop-opacity="0"/>
      <stop offset="0.7" stop-color="{fog_colour}" stop-opacity="{fog * 0.8:.2f}"/>
      <stop offset="1" stop-color="{fog_colour}" stop-opacity="0"/>
    </linearGradient>""")
        if RIDGES[i][5] > 0:
            defs.append(f"""
    <filter id="soft{i}" x="-2%" y="-10%" width="104%" height="120%">
      <feGaussianBlur stdDeviation="{RIDGES[i][5]}"/>
    </filter>""")
    if blurred:
        defs.append("""
    <filter id="blur" x="-5%" y="-5%" width="110%" height="110%">
      <feGaussianBlur stdDeviation="48"/>
    </filter>""")

    body = [f'<rect width="{W}" height="{H}" fill="url(#sky)"/>',
            f'<rect width="{W}" height="{H}" fill="url(#sun)"/>']
    if at_night:
        # Faint stars above the ridges, and the moon in the haze.
        rnd = random.Random(5)
        stars = []
        for _ in range(140):
            x, y = rnd.uniform(0, W), rnd.uniform(0, H * 0.4) ** 1.0
            stars.append(f'<circle cx="{f(x)}" cy="{f(y)}" r="{rnd.uniform(0.8, 2.0):.1f}" '
                         f'fill="#E8EEF4" opacity="{rnd.uniform(0.15, 0.7) * (1 - y / (H * 0.45)):.2f}"/>')
        body.append("".join(stars))
        body.append(f'<circle cx="{f(SUN[0])}" cy="{f(SUN[1])}" r="38" fill="#EEF2F4" opacity="0.92"/>')
    for i, (base_f, amp, heights, spacing, colour, soft, fog) in enumerate(RIDGES):
        ground, trees = ridge(i, base_f, amp, heights, spacing)
        filt = f' filter="url(#soft{i})"' if soft > 0 else ""
        fill = night(colour) if at_night else colour
        body.append(f'<g fill="{fill}"{filt}><path d="{ground}"/><path d="{trees}"/></g>')
        if fog > 0:
            # A smooth haze over this ridge's foot, then wisps of fog on it.
            body.append(f'<rect y="{base_f * H - 140:.0f}" width="{W}" height="230" fill="url(#haze{i})"/>')
            body.append(f'<rect width="{W}" height="{H}" filter="url(#fog{i})" mask="url(#fogmask{i})"/>')
    # Light falling through the mist from the sun, in soft shafts.
    rnd = random.Random(3)
    rays = []
    for _ in range(9):
        a = math.radians(rnd.uniform(102, 150))
        spread = math.radians(rnd.uniform(1.2, 3.2))
        length = H * rnd.uniform(0.55, 0.85)
        p1 = (SUN[0] + length * math.cos(a - spread), SUN[1] + length * math.sin(a - spread))
        p2 = (SUN[0] + length * math.cos(a + spread), SUN[1] + length * math.sin(a + spread))
        rays.append(f'<path d="M{f(SUN[0])} {f(SUN[1])}L{f(p1[0])} {f(p1[1])}L{f(p2[0])} {f(p2[1])}Z" '
                    f'fill="#FFF8E6" opacity="{rnd.uniform(0.04, 0.09):.2f}"/>')
    if not at_night:
        body.append(f'<g filter="url(#rays)">{"".join(rays)}</g>')
    body.append(f'<rect width="{W}" height="{H}" fill="url(#vignette)"/>')
    body.append(f'<rect width="{W}" height="{H}" filter="url(#grain)"/>')

    inner = "\n  ".join(body)
    if blurred:
        # Scale the scene up a little so the blur's edge falloff (it samples
        # transparency outside the canvas) lands off-screen.
        inner = (f'<g filter="url(#blur)"><g transform="translate({W / 2} {H / 2}) scale(1.12) '
                 f'translate({-W / 2} {-H / 2})">\n  {inner}\n  </g></g>\n'
                 f'  <rect width="{W}" height="{H}" fill="#0B1A13" opacity="0.42"/>')
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}">
  <title>{"Grove at night" if at_night else "Grove"}</title>
  <!-- Original artwork for Fern. Generated by assets/wallpapers/gen-wallpapers.py. -->
  <defs>{''.join(defs)}
  </defs>
  {inner}
</svg>
"""


def main():
    (OUT / "grove.svg").write_text(scene(False))
    (OUT / "grove-night.svg").write_text(scene(False, at_night=True))
    (OUT / "grove-blurred.svg").write_text(scene(True))
    print(f"wrote {OUT}/grove.svg, grove-night.svg and grove-blurred.svg")


if __name__ == "__main__":
    main()
