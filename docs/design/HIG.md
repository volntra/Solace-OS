# Koi Human Interface Guidelines

**Status: outline only.** The values below are the fixed starting points
from CLAUDE.md. Everything marked *(Phase 6)* is defined, implemented in
`de/design/`, and documented with screenshots in Phase 6. The GRUB theme
(Phase 2) uses the palette and type from here.

## Principles

1. **Calm.** Few colors, generous space, motion that settles and doesn't
   bounce for attention.
2. **Consistent.** Every surface comes from the shared design-system
   module; no app styles its own controls.
3. **Ours.** Original art and permissively licensed fonts and symbols
   only. Nothing from Apple, and no upstream Linux branding.
4. **Invisible plumbing.** The user never sees the Linux underneath: no
   terminal flashes, unthemed dialogs, or raw error text.

## Foundations

| Area | Fixed now | To define *(Phase 6)* |
|---|---|---|
| Spacing | 4 px grid | named steps |
| Type | Inter (UI), JetBrains Mono (mono) | size/weight/leading scale |
| Symbols | Phosphor or Lucide, plus our own | final choice + sizes |
| Icon shape | continuous-curvature squircle (superellipse) | exponent, sizes |
| Color | light + dark palettes + accent; every pair passes WCAG AA | tokens, contrast table |
| Materials | window, sidebar, popover, menu, HUD, sheet = blur + tint + noise | blur radii, tint alphas |
| Radii | | window, control, popover radii; corner style presets |
| Motion | critically damped springs, never fixed durations | spring presets; reduce-motion mapping |

## Controls *(Phase 6)*

Buttons, toggles, checkboxes, segmented controls, sliders, steppers, text
fields, search fields, popovers, sheets, alerts, menus, sidebars, toolbars
with unified titlebars, tables, outline views, column views.

## Windows *(Phase 5)*

Traffic-light buttons top-left (close / minimize / zoom), glyphs on hover,
dimmed when inactive; shadows tied to focus.
