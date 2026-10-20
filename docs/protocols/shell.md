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
- `set_wallpaper(path)`: the desktop picture (a PNG the compositor reads
  itself), crossfaded from the one before over 0.88 s. The shell picks the
  light or dark one of the user's pair.
- `set_color_temperature(kelvin)`: Night Shift. The DRM backend sets every
  display's gamma ramps to the white point of a black body at that
  temperature (`de/comp/src/nightshift.rs`), so it costs nothing per
  frame; 0 or 6500 and above is neutral. The shell decides when, from the
  `nightshift` settings (by hand, or a schedule: sunset is 19:00 to 7:00
  until location services exist), and resends it after a restart. A
  nested compositor has no ramps and stays neutral.
- `screenshot(id, x, y, width, height)`: the compositor draws the next
  frame of that rectangle (or, when empty, the whole display under the
  pointer) again offscreen without the pointer and answers
  `screenshot_done(id, fd, width, height)` with RGBA pixels in a memfd, or
  `screenshot_failed(id)`. Only the shell can ask (the global is
  trusted-only), so apps cannot read the screen this way; they get the
  portal in Phase 11. The shell saves PNGs in `~/Pictures/Screenshots`
  ("Screenshot 2026-10-20 at 21.04.07.png", never overwriting) and posts
  a notification with a thumbnail. Keys: Shift+Super+3 (and Print) the
  display, Shift+Super+4 a dragged rectangle; also in Search.
- `app_menu(service, path)` (event): where the active window's menus are,
  sent on bind and whenever it changes; empty strings when it has none.

## The global menu

Apps say where their menus are with KDE's `org_kde_kwin_appmenu`
(offered to every client, for its own surfaces): a session-bus service
and object path exporting `com.canonical.dbusmenu`. The compositor sends
the active window's address as `app_menu`; the menu bar reads the layout
(`GetLayout`), shows its top-level menus after the bold app name, sends
`AboutToShow` before opening one and `Event(id, "clicked")` on a choice.
Apps without one get our File and Window menus. The address is the app's
claim: the shell bounds what it reads (6 levels, 200 entries a level) and
never acts on a menu except by sending the app its own clicks. Left and
Right move between the bar's menus, as do hover and click.

## Status icons

The shell is the session's `org.kde.StatusNotifierWatcher` and its only
host. Items register by service name or object path (the caller's unique
name is then the service); each shows in the menu bar from its icon
name, icon theme path or pixmap (at most 1024x1024). A click opens the
item's dbusmenu, or calls `Activate(x, y)` when it has none. Items leave
when their owner leaves the bus.
