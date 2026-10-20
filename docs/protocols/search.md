# Search: `org.fern.Search1`

`fern-indexd` (de/indexer) indexes the names of the files and folders under
the user's home folder, so the shell's Search (Super+Space) can find them.
It runs as the user, one per session.

| Bus | Name | Path | Index |
|---|---|---|---|
| session | `org.fern.Search` | `/org/fern/Search` | `~/.cache/fern/index.sqlite` |

## Methods

- `Query(text s, limit u) -> a(ssb)`: files and folders whose names have
  words starting with every word of `text`, best first, as (path, name,
  is a folder). At most 50, whatever `limit` says. Names that start with
  the text come first, then FTS5's relevance, then shorter paths. Words
  are split at anything but letters, digits and `_`, and matched without
  case or accents ("cafe" finds "Café"). What the user types is never
  read as query syntax.

## Properties

- `Ready b`: the first crawl since the daemon started has finished. Before
  that, results come from the last run's index and may miss recent
  changes.

## What is indexed

- Names only, not contents.
- Hidden entries (a name starting with a dot) and everything below them
  are left out; so are paths that are not UTF-8.
- Links are entries, never followed.
- At most 2,000,000 entries. Past that the index is partial and says so
  in the log.

## Keeping current

At start the daemon crawls the whole home folder, then forgets entries
the crawl did not see (removed while it was not running). After that,
inotify follows every folder: made, deleted, renamed and moved-in entries
change the index at once, and a folder moved in is crawled with
everything inside. If the kernel drops events (queue overflow), it crawls
again. Running out of inotify watches (`fs.inotify.max_user_watches`) is
logged; changes below the folders it could not watch are then only picked
up by the next start.

fanotify would need CAP_SYS_ADMIN for directory events; the indexer runs
as the user, so it uses inotify.

## Privacy

The index lives in the user's own cache folder and is served only on the
user's session bus.
