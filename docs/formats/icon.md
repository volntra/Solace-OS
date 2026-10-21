# Adaptive icon format (`.fernicon`)

An icon is a directory named `<id>.fernicon/`. `<id>` is the app's id
(its desktop entry's name without `.desktop`, e.g. `org.fern.Files`), or
a name from the icon set for folders, volumes and file types
(`folder.documents`, `drive.external`, `file.pdf`, ...; see
`de/icons/set/README.md`).

```
org.fern.Files.fernicon/
  icon.toml            metadata (below)
  background.svg       the shape's fill
  foreground.svg       the glyph, its fills tagged by role
  dark-background.svg  optional: the designer's dark mode
  dark-foreground.svg  optional
```

Icons are looked up by id in `$XDG_DATA_HOME/fern/icons/`, then in each
of `$XDG_DATA_DIRS` + `/fern/icons/` (the system set is
`/usr/share/fern/icons/`). The first match wins, so a user can override
an icon.

## icon.toml

```toml
format = 1
key = "#2D64C8"        # the icon's main colour (required): the hue dark
                       # mode keeps for neutral glyphs
[roles]                # colours of the tagged fills in the default look
primary = "#FFFFFF"
secondary = "#CFE0FF"
highlight = "#FFC94D"

[tint]                 # which layers the Tinted, Wallpaper and Clear
background = true      # modes may recolour (default true): a layer that
foreground = true      # says false keeps its own colours in every mode
```

Colours are `#rrggbb`. Every role named in `foreground.svg` must have a
colour in `[roles]`.

## The SVG layers

- Both layers are drawn on a 1024 × 1024 canvas (`viewBox="0 0 1024
  1024"`) and both are clipped to the icon shape, so the background
  should fill the whole square.
- Paths and shapes only: no `<text>`, no raster images, no external
  references, no scripts (the engine renders with resvg, without fonts).
- In the foreground, an element with `data-role="primary"`,
  `"secondary"` or `"highlight"` gets that role's colour: its `fill` and
  `stroke` attributes (each one present and not `none`), and the
  `stop-color` of a `<stop>`. Colours given in `style="…"` are not
  rewritten, so tagged elements use attributes. Untagged elements keep
  their colours (shadows, fixed details).

## The shape

Every icon is drawn on the same plate: a superellipse
(|x|ⁿ + |y|ⁿ = 1) whose curvature changes continuously, centred in the
icon's square. Its exponent and size are defined once, in
`de/design/shape/squircle.toml`, and the engine is the only thing that
draws it. The engine adds the plate's soft shadow, a faint outline and a
light top edge.

## Render modes

The engine (`de/icons`, fern-icons) renders an icon at a pixel size in
one of five modes:

| Mode | Background | Foreground |
|---|---|---|
| Default | as designed | as designed, roles from `[roles]` |
| Dark | `dark-background.svg`, or a near-black gradient with a hint of the key hue | `dark-foreground.svg`, or the design with each role's lightness clamped to stay readable; neutral roles take the key hue |
| Tinted | a dark gradient in the accent's hue | a luminance mask coloured with a light accent tone |
| Wallpaper | as Tinted, with the accent taken from the wallpaper | as Tinted |
| Clear | frosted glass: translucent white | a white luminance mask |

Colour work is done in HCT (hue, chroma, tone; the Material Color
Utilities model, Apache-2.0, through the `material-colors` crate). The
wallpaper's accent is the first colour of that library's scoring of a
quantized (Celebi) copy of the wallpaper.

## Legacy icons

Apps without a `.fernicon` keep their freedesktop icon (PNG or SVG). It
is placed on the plate:

- Default and Dark: a pale (Dark: deep) plate in the icon's dominant
  colour, the icon at 70% in the middle. An icon that already fills its
  square is clipped to the shape instead.
- Tinted, Wallpaper and Clear: the icon becomes a luminance mask, tinted
  like a native foreground, on that mode's background.

## Cache

Rendered icons are kept in `~/.cache/fern/icons/<sha256>.png`, keyed by
the engine's version, a hash of the icon's files (or the legacy file),
the mode, the accent (for the modes that use it) and the pixel size.
Deleting the folder is always safe.
