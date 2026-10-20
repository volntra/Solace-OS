# Settings: `org.fern.Settings1`

fern-settingsd owns every setting (CLAUDE.md 6.10). Components read and
change settings only through it, never through its files.

Two instances run:

| Instance | Bus | Name | Files | Domains |
|---|---|---|---|---|
| `fern-settingsd` | session | `org.fern.Settings` | `~/.config/fern/<domain>.toml` | appearance, wallpaper, dock, menubar, notifications |
| `fern-settingsd --system` | system | `org.fern.Settings` | `/etc/fern/<domain>.toml` | boot |

Object `/org/fern/Settings`, interface `org.fern.Settings1`.

## Methods

| Method | Signature | |
|---|---|---|
| `Domains` | `() → as` | the domains this instance owns |
| `Describe` | `(s domain) → a(sssv)` | each key: name, type, description, default |
| `Get` | `(s domain, s key) → v` | the value (the default when never set) |
| `GetAll` | `(s domain) → a{sv}` | every key of a domain |
| `Set` | `(s domain, s key, v value)` | validate, save, apply, announce |
| `Reset` | `(s domain, s key)` | back to the default (the key leaves the file) |

Signal `Changed(s domain, s key, v value)` is sent after a value changes. To
watch one domain, match on `arg0`. A client reads its domain with `GetAll`
after subscribing, so it misses nothing.

## Values

Every value is `b` (bool), `x` (int64), `s` (text) or `as` (list of text).
`Set` also accepts the other integer types for int keys. `Describe` gives
these type strings:

| Type | Accepts |
|---|---|
| `bool` | true or false |
| `int MIN..MAX` | an integer in the range |
| `choice A\|B\|C` | one of the words |
| `text` | printable text, at most 256 bytes |
| `id` | `[A-Za-z0-9._-]`, at most 64 bytes |
| `path` | an absolute path |
| `accent` | a preset (`fern blue purple pink orange amber graphite`), `wallpaper`, or `#rrggbb` |
| `list ITEM` | a list of ITEM values |

A `?` after the type means the empty string is allowed and means "not set".
An unset optional value is left out of the file.

## Errors

- `org.freedesktop.DBus.Error.InvalidArgs`: an unknown domain or key, or a
  value of the wrong type or out of range. Nothing was changed.
- `org.freedesktop.DBus.Error.AccessDenied`: a system setting changed by
  someone who is not root.
- `org.freedesktop.DBus.Error.Failed`: the file could not be written, or
  applying the change failed. The old file is restored and the message
  says why.

## Applying

- **boot.** After a change to the boot domain, the system instance runs
  `/usr/bin/spk boot-menu`. That regenerates grub.cfg from
  `/etc/fern/boot.toml`, the file fern-bootgen reads through spk. If it
  fails (for example, the ESP is too full), the change is undone. spk also
  writes `boot.default` itself (`spk rollback`); the daemon reads system
  files again before every call and announces what changed.
- **Everything else.** Components watch `Changed` and apply the new value
  themselves.

## Files

Each domain has one TOML file holding only the keys that were set. Writes
are atomic: write to a temporary file, fsync, then rename. A value of the
wrong type in a file is ignored with a warning, and the key reads as its
default. Keys the schema does not know are kept unchanged, so an older
daemon never drops a newer one's settings.

## Access

The session bus belongs to the user. On the system bus, `Set` and `Reset`
are allowed only for uid 0 (and for the daemon's own uid, which is how the
tests run it). polkit replaces this rule once it is in the image. Reading
is open to everyone.

## Schema

The schema lives in `de/settingsd/src/schema.rs`, and
`fern-settings describe DOMAIN` prints it. A key exists only once a
component reads it. More domains (wm, input, displays, performance) arrive
with the Settings app in Phase 8.

## Command line

```
fern-settings list [DOMAIN]
fern-settings describe DOMAIN
fern-settings get DOMAIN KEY
fern-settings set DOMAIN KEY VALUE     # lists: comma-separated
fern-settings reset DOMAIN KEY
fern-settings watch [DOMAIN]
fern-settings --system set boot timeout 3
```
