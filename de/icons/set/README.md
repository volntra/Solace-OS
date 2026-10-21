# Fern's icon set

Original icons in the adaptive format (docs/formats/icon.md), generated
by `gen-icons.py` into `icons/` (edit the script, then run it; the
output is committed). Installed to `/usr/share/fern/icons/` by the
`fern-icons` package. `just icon-sheet` renders every icon in every mode
to `build/icon-sheet.png` for review.

| Ids | What |
|---|---|
| `org.fern.Files`, `org.fern.Settings`, `org.fern.Terminal`, `org.fern.Installer` | built-in apps |
| `fern.app` | any app with no icon at all |
| `folder`, `folder.home`, `folder.applications`, `folder.users`, `folder.desktop`, `folder.documents`, `folder.downloads`, `folder.pictures`, `folder.music`, `folder.movies`, `folder.public`, `folder.system`, `folder.volumes` | folders |
| `trash`, `trash.full` | the Trash |
| `drive.internal`, `drive.external`, `drive.usb`, `drive.memory`, `drive.optical`, `drive.network` | volumes |
| `file`, `file.text`, `file.document`, `file.code`, `file.image`, `file.audio`, `file.video`, `file.pdf`, `file.archive`, `file.spreadsheet`, `file.presentation`, `file.font`, `file.script`, `file.package`, `file.app`, `file.disk-image`, `file.web` | file types |

Design rules the generator follows:

- Backgrounds fill the whole 1024 square with a vertical gradient,
  lighter at the top, and a soft light from above.
- Glyphs are light (white or near it): the Tinted, Wallpaper and Clear
  modes turn lightness into opacity, so light shapes stay solid and dark
  detail becomes a cut-out.
- Shapes keep inside the plate's safe area (about 200 to 824).
- Small glyphs on folders, drives and documents are Lucide symbols
  (ISC, `assets/symbols/lucide/LICENSE`); the rest is drawn here.
