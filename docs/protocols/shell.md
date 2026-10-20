# The shell and the compositor

The desktop shell (fern-shell: menu bar, dock, search, notifications,
control center) is a client that fern-comp starts itself, with
`--shell PATH`, on a socket pair. It is restarted if it exits, up to 5
times in 30 s. Being started that way makes it *trusted*, and only
trusted clients see these globals:

| Global | What the shell uses it for |
|---|---|
| `zwlr_layer_shell_v1` (v4) | its surfaces: the menu bar and dock (top layer), banners (overlay), panels |
| `zwlr_foreign_toplevel_manager_v1` (v3) | the dock's running apps; `set_rectangle` on a dock surface is where a window minimizes to |
| `fern_shell_v1` (v1, ours) | rounded blur behind its surfaces, global shortcuts |

An app sees none of them. Otherwise it could cover the screen, fake the
menu bar, or close other apps' windows.

## Layers

From the bottom up, fern-comp draws:

1. the wallpaper
2. the `background` and `bottom` layers
3. windows
4. the `top` layer
5. the window manager's overlay (the app switcher goes over the dock)
6. the `overlay` layer

The pointer reaches the `top` and `overlay` layers before any window. A
click there goes straight to the shell and never to the window manager,
so it does not change window focus.

Exclusive zones, the rooms the menu bar and a dock that does not hide keep
for themselves, make each output's *work area*. The window manager receives
it as `fern_wm_v1.work_area`. New windows are placed in it, and zoom and
tiling fill it. When it changes, tiled and zoomed windows follow.

Keyboard focus:

- `exclusive`: a surface on the `top` or `overlay` layer that asks for it
  (an open search panel) keeps the keyboard while it is mapped. Focusing
  a window meanwhile raises and activates the window but leaves the
  keyboard with the panel. When the panel goes, the keyboard returns.
- `on_demand`: the surface gets the keyboard when clicked.

## `fern_shell_v1`

XML: `de/protocols/fern-shell-v1.xml`.

- `set_blur(surface, x, y, width, height, radius)`: blur what lies behind
  this rounded rectangle of a layer surface, from the next frame on (the
  standard material blur, HIG: Materials). The shell draws its tint over
  it. One rectangle per surface; `unset_blur` removes it.
- `bind_key(id, modifiers, keysym)`: a global shortcut. Modifier bits:
  shift 1, ctrl 2, alt 4, super 8. On a press the shell gets
  `key_binding(id)` and the focused app gets nothing. Shell shortcuts come
  before the window manager's; while the window manager holds the keyboard
  (Overview, the switcher) they do not fire. `unbind_key(id)` removes one.
