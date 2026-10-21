import QtQuick
import QtTest
import Fern.Design

// Adaptive icons in QML: URLs, the image provider (drawing the repo's icon
// set), the icon mode following light and dark, the wallpaper's accent,
// and WCAG AA for the tones a wallpaper accent is used at, for any hue.
Item {
    width: 128
    height: 128

    Image { id: picture; width: 64; height: 64; sourceSize: Qt.size(64, 64) }
    WallpaperAccent { id: accent }

TestCase {
    name: "Icons"
    when: windowShown

    function lum(c) {
        const f = v => v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4)
        return 0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b)
    }
    function contrast(a, b) {
        const x = lum(a), y = lum(b)
        return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05)
    }

    function test_urls() {
        compare(Icons.url("org.fern.Files", "", "tinted", "#B4430F"), "image://fernicon/tinted/b4430f/org.fern.Files")
        compare(Icons.url("x", "file:///usr/share/icons/a b.png", "dark", "#000000"),
                "image://fernicon/dark/000000/x/usr/share/icons/a%20b.png")
        // An unknown mode is the default look; no id is the generic app.
        compare(Icons.url("", "", "sepia", "#2F7A50"), "image://fernicon/default/2f7a50/fern.app")
        // Relative paths are not icon files.
        compare(Icons.url("x", "icons/a.png", "default", "#000000"), "image://fernicon/default/000000/x")
    }

    function load(url) {
        picture.source = ""
        picture.source = url
        tryCompare(picture, "status", Image.Ready, 10000)
        waitForRendering(picture)
        return grabImage(picture)
    }

    function test_the_provider_draws_icons() {
        // Files in Default: the white folder front in the middle, blue
        // around it, the corners clear.
        let img = load(Icons.url("org.fern.Files", "", "default", "#000000"))
        const mid = img.pixel(32, 38)
        verify(mid.r > 0.9 && mid.b > 0.9, "a white folder: " + mid)
        const side = img.pixel(8, 32)
        verify(side.b > side.r + 0.3, "a blue plate: " + side)
        // The corner is outside the plate: the window's white shows.
        const corner = img.pixel(0, 0)
        verify(corner.r > 0.95 && corner.b > 0.95, "a clear corner: " + corner)
        // Dark: the plate turns near black.
        img = load(Icons.url("org.fern.Files", "", "dark", "#000000"))
        verify(lum(img.pixel(8, 32)) < 0.05, "a dark plate: " + img.pixel(8, 32))
        // An app with no icon gets the generic one, not nothing.
        img = load(Icons.url("org.example.NoSuchApp", "", "default", "#000000"))
        verify(img.pixel(32, 32).a > 0.9)
    }

    function test_icon_mode_follows_the_appearance() {
        Theme.appearance.icon_mode = "auto"
        Theme.appearance.theme = "light"
        compare(Theme.iconMode, "default")
        Theme.appearance.theme = "dark"
        compare(Theme.iconMode, "dark")
        Theme.appearance.icon_mode = "tinted"
        compare(Theme.iconMode, "tinted")
        verify(Theme.icon("org.fern.Files", "").startsWith("image://fernicon/tinted/"))
        Theme.appearance.icon_mode = "auto"
        Theme.appearance.theme = "light"
    }

    function test_wallpaper_accent() {
        accent.source = FernTest.greenPicture
        tryCompare(accent, "ready", true, 10000)
        verify(accent.color.g > accent.color.r && accent.color.g > accent.color.b, "green: " + accent.color)
        // Unreadable: keeps what it had.
        accent.source = "/nonexistent.png"
        wait(200)
        verify(accent.color.g > accent.color.r)
    }

    function test_wallpaper_accent_tones_pass_aa_for_any_hue() {
        const ink = Theme.ink
        for (const c of ["#D03030", "#E0B020", "#30A050", "#20A0B0", "#3050D0", "#B030B0", "#808080", "#F0F0F0", "#101010"]) {
            const fill = Icons.tone(c, 40), darkFill = Icons.tone(c, 80), text = Icons.tone(c, 35)
            verify(contrast(Qt.color("#FFFFFF"), fill) >= 4.5, c + ": white on tone 40 " + fill)
            verify(contrast(ink, darkFill) >= 4.5, c + ": ink on tone 80 " + darkFill)
            verify(contrast(text, Theme.paper) >= 4.5, c + ": tone 35 on paper " + text)
        }
    }

    function test_wallpaper_is_an_accent() {
        Theme.appearance.accent = "wallpaper"
        compare(Theme.accentName, "wallpaper")
        Theme.appearance.accent = "fern"
    }
}
}
