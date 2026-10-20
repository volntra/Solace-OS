# fern_wm_v1: the core and the window manager

Wire definition: `de/protocols/fern-wm-v1.xml` (bindings in the
`fern-wm-proto` crate). This document is the source of truth for
behaviour.

## Roles

**fern-comp (the core)** owns the screen, input devices, every client
protocol (xdg-shell, linux-dmabuf, blur, ...), rendering and animation. It
makes no policy decisions while a window manager runs.

**fern-wm (the window manager)** owns all policy: where windows go, their
size and states, stacking, focus, decorations, Spaces, Overview, the app
switcher, key bindings. It never touches a client directly.

## Connection

The core starts the window manager itself: it makes a socket pair, gives
the window manager one end as standard input and sets `WAYLAND_SOCKET=0`
(the `--wm PATH` option or `FERN_WM` name the binary; by default the
`fern-wm` next to `fern-comp`). The `fern_wm_v1` global is visible only to
that client; nothing on the public socket can see or bind it. Only one
window manager is bound at a time.

## Identity

Windows and outputs are integer ids chosen by the core, never reused while
the core runs. A window id is assigned when the client creates its
toplevel, before it has drawn anything.

## Startup and the restart contract

On bind the core describes the whole desktop:

1. `output` for each output;
2. for each window: `window`, `window_hints`, `window_size` (if it has
   drawn), then `restore` with where it is now, its states, whether it is
   visible, and the data the previous window manager stored with `save`;
3. `restore_wm` with what `save_wm` stored (empty the first time);
4. `done`.

Between bind and `done`, `window_size` describes windows that are already
placed; a window manager must not treat it as a new window.

If the window manager exits or crashes, the core:

- keeps every window exactly where it is, with its size, states and
  stacking; nothing on screen moves;
- drops its key bindings and grabs, and clears its overlay (so a crash in
  the app switcher or Overview never leaves the user stuck);
- places new windows itself (centred, a little high, cascading) and
  focuses on click, so the session stays usable;
- starts a new window manager after 300 ms, and gives up after 5 exits in
  30 seconds.

`save` and `save_wm` exist for this: anything the window manager needs to
pick up where it left off (which Space a window is on, a window's size
before it was zoomed, the current Space) goes there.

## Transactions

Every request takes effect at `commit`, all together and in order, so the
window manager can move, restack and refocus without intermediate frames.

## Windows

- `window_size` reports the content size the client committed; the first
  one means the window has drawn and can be shown.
- `configure` sends the client a size (0 x 0: the client chooses) and its
  states (activated, maximized, fullscreen, tiled edges, resizing).
- `place` says where the window's frame goes (its top-left, titlebar
  included) and how it is drawn: `scale` and `alpha` change only the
  picture, never the client's size. `motion` picks the spring the core
  animates with (`instant`, `snappy`, `smooth`, `gentle`; HIG presets).
- `visible`: hidden windows are neither drawn nor hit by the pointer (a
  minimized window, a window on another Space).
- `stack` gives the order of all windows, bottom to top.
- `focus` sets keyboard focus; activation is a `configure` state.
- `decoration`: `titlebar` asks the core to draw our titlebar with traffic
  lights above the content; `none` shows the content alone.
- Clients' own requests (move, resize, maximize, fullscreen, minimize,
  activate) arrive as `window_request`. A client's interactive move or
  resize is carried out by the core directly; the window manager hears
  `move_update` / `move_end` and `resize_end`, and can snap the result.
- `begin_move` / `begin_resize` start the same interactive operations from
  the window manager (a drag on the titlebar, a resize edge).

## Input

- `pointer_button` reports every button press and release with the window
  and the part under the pointer (content, titlebar, one of the three
  buttons, a resize edge, the desktop), the modifiers, and whether it was a
  double click. Clients still get presses on their content unless the
  window manager grabbed the pointer.
- `grab_pointer`: while set, clients get no pointer events; the window
  manager gets `pointer_motion` and every button (Overview, switcher).
- `bind` registers a key combination (modifier bits plus the unshifted
  keysym); a press arrives as `binding` and neither it nor its release
  reaches a client.
- `grab_keyboard`: while set, every key goes to the window manager as
  `key` (press and release, with modifiers), none to clients.
- The core keeps for itself: Ctrl+Alt+F1..F12 (virtual terminals) and
  Ctrl+Alt+Backspace (quit).

## Overlay

Drawn above all windows, replaced as a whole at commit after
`overlay_clear`: rounded rectangles (`overlay_rect`), the wallpaper in a
rounded rectangle (`overlay_wallpaper`), a window's picture scaled into a
rectangle (`overlay_mirror`; the window itself stays where it is), and
text (`overlay_text`). Overview, the Spaces strip, the app switcher and
the tiling preview are drawn with these.

## What is implemented

See docs/STATUS.md for which parts of this the core and fern-wm carry out
today.
