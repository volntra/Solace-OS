# Symbols

UI glyphs for the shell and apps (CLAUDE.md 3: "Phosphor or Lucide
symbols"). We chose Lucide (ISC licence, `lucide/LICENSE`); see
docs/DECISIONS.md.

`lucide/` holds the icons we use, copied unchanged from `lucide-static`
1.60.0 except that the licence comment line is dropped (the licence file
sits beside them). They are drawn with `stroke="currentColor"`. The design
system's `image://symbol/NAME?color=#rrggbb` provider recolours them, so
they follow light and dark mode and the accent.

To add one, copy `icons/NAME.svg` from the same `lucide-static` release
into `lucide/`.
