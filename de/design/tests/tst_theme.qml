import QtQuick
import QtTest
import Fern.Design

// WCAG AA (4.5:1) for every text/background pair the design system
// defines, in light and dark mode, for every accent; text on translucent
// materials is checked over both white and black backdrops (the worst
// cases of what can be behind them).
TestCase {
    name: "Theme"

    function lum(c) {
        const f = v => v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4)
        return 0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b)
    }
    function contrast(a, b) {
        const x = lum(a), y = lum(b)
        return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05)
    }
    function over(top, bottom) {
        return Qt.rgba(top.r * top.a + bottom.r * (1 - top.a),
                       top.g * top.a + bottom.g * (1 - top.a),
                       top.b * top.a + bottom.b * (1 - top.a), 1)
    }
    function aa(fg, bg, what) {
        const r = contrast(fg, bg)
        verify(r >= 4.5, what + ": " + r.toFixed(2) + ":1 (" + fg + " on " + bg + ")")
    }

    function checkMode(dark) {
        Theme.appearance.theme = dark ? "dark" : "light"
        compare(Theme.dark, dark)
        const mode = dark ? "dark" : "light"
        for (const bg of ["window", "surface", "sidebar"]) {
            aa(Theme.text, Theme[bg], mode + " text on " + bg)
            aa(Theme.secondaryText, Theme[bg], mode + " secondary text on " + bg)
            aa(Theme.accentText, Theme[bg], mode + " accent text on " + bg)
            aa(Theme.destructive, Theme[bg], mode + " destructive on " + bg)
        }
        for (const m of ["menubar", "popover", "menu", "sheet", "sidebar"]) {
            for (const backdrop of [Qt.rgba(1, 1, 1, 1), Qt.rgba(0, 0, 0, 1)]) {
                aa(Theme.text, over(Theme.material(m), backdrop), mode + " text on " + m + " over " + backdrop)
            }
        }
        aa(Qt.rgba(1, 1, 1, 1), over(Theme.material("hud"), Qt.rgba(1, 1, 1, 1)), "white on the HUD")
    }

    function test_text_pairs() {
        checkMode(false)
        checkMode(true)
    }

    function test_accents() {
        for (const name of Object.keys(Theme.accents)) {
            for (const dark of [false, true]) {
                Theme.appearance.theme = dark ? "dark" : "light"
                Theme.appearance.accent = name
                compare(Theme.accentName, name)
                aa(Theme.textOnAccent, Theme.accent, name + (dark ? " dark" : " light") + " label on the accent")
                aa(Theme.accentText, Theme.window, name + (dark ? " dark" : " light") + " accent text")
            }
        }
    }

    function test_unknown_accent_falls_back() {
        Theme.appearance.accent = "teal"
        compare(Theme.accentName, "fern")
        Theme.appearance.accent = "#336699"
        compare(Theme.accentName, "custom")
        Theme.appearance.accent = "fern"
    }

    function test_text_size_scales_type() {
        Theme.appearance.text_size = "default"
        compare(Theme.body, 13)
        Theme.appearance.text_size = "larger"
        verify(Theme.body > 13)
        Theme.appearance.text_size = "default"
    }

    function test_reduce_motion_stops_movement() {
        Theme.appearance.reduce_motion = true
        compare(Theme.smooth, 0)
        Theme.appearance.reduce_motion = false
        verify(Theme.smooth > 0)
    }

    function cleanupTestCase() {
        Theme.appearance.theme = "light"
    }
}
