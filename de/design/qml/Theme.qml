pragma Singleton
import QtQuick
import Fern.Design

// Every colour, size and material the desktop draws with (HIG:
// docs/design/HIG.md). Components read tokens from here, never literals.
// Light/dark, the accent, text size, reduce motion and reduce transparency
// come from the user's appearance settings and change live.
QtObject {
    id: theme

    readonly property SettingsDomain appearance: SettingsDomain {
        domain: "appearance"
        defaults: ({ theme: "light", accent: "fern", reduce_motion: false,
                     reduce_transparency: false, text_size: "default", icon_mode: "auto" })
    }
    readonly property SettingsDomain wallpaperSettings: SettingsDomain {
        domain: "wallpaper"
        defaults: ({ light: "/usr/share/fern/wallpapers/grove.png",
                     dark: "/usr/share/fern/wallpapers/grove-night.png" })
    }

    // --- Mode -----------------------------------------------------------

    // "auto" is dark from 19:00 to 7:00 (until sunrise and sunset come
    // with location services).
    property int hour: new Date().getHours()
    readonly property Timer clock: Timer {
        interval: 60000; running: true; repeat: true
        onTriggered: theme.hour = new Date().getHours()
    }
    readonly property string mode: appearance.theme ?? "light"
    readonly property bool dark: mode === "dark" || (mode === "auto" && (hour >= 19 || hour < 7))
    readonly property bool reduceMotion: appearance.reduce_motion === true
    readonly property bool reduceTransparency: appearance.reduce_transparency === true

    // --- Brand palette (HIG: Brand) -------------------------------------

    readonly property color ink: "#101512"
    readonly property color forest: "#173D2E"
    readonly property color pine: "#245C43"
    readonly property color fern: "#2F7A50"
    readonly property color leaf: "#7CC48A"
    readonly property color moss: "#E3F0E4"
    readonly property color paper: "#F6F7F3"

    // --- Accent ---------------------------------------------------------

    // Each preset as [light fill, dark fill, light-mode accent text].
    // Fills carry white text in light mode and Ink in dark mode; every pair
    // passes WCAG AA (tests/tst_theme.qml).
    readonly property var accents: ({
        fern:     ["#2F7A50", "#7CC48A", "#245C43"],
        blue:     ["#2D64C8", "#7BA7F5", "#2756AD"],
        purple:   ["#7048C2", "#B39CF0", "#6340AD"],
        pink:     ["#B83E70", "#F09AC0", "#A33763"],
        orange:   ["#B4430F", "#F5A06E", "#A03B0D"],
        amber:    ["#8A5A0C", "#E8B85C", "#7A500B"],
        graphite: ["#525A57", "#B2BAB6", "#4A514E"]
    })
    readonly property string accentName: {
        const a = appearance.accent ?? "fern"
        if (a === "wallpaper")
            return a
        return accents[a] !== undefined ? a : (a.startsWith("#") ? "custom" : "fern")
    }
    // "wallpaper": the colour the wallpaper suggests, at the tones the
    // presets use (HCT tone 40 under white text, 80 under Ink, text 35),
    // which keeps every pair WCAG AA whatever the hue.
    readonly property color accent: accentName === "wallpaper"
        ? Icons.tone(wallpaperAccent.color, dark ? 80 : 40)
        : accentName === "custom"
        ? (dark ? Qt.lighter(appearance.accent, 1.45) : appearance.accent)
        : accents[accentName][dark ? 1 : 0]
    readonly property color textOnAccent: dark ? ink : "#FFFFFF"
    readonly property color accentText: accentName === "wallpaper"
        ? Icons.tone(wallpaperAccent.color, dark ? 80 : 35)
        : accentName === "custom"
        ? accent : accents[accentName][dark ? 1 : 2]
    // The accent as chosen, before light/dark: what Tinted icons take.
    readonly property color accentBase: accentName === "wallpaper" ? wallpaperAccent.color
        : accentName === "custom" ? appearance.accent : accents[accentName][0]

    // --- Wallpaper ------------------------------------------------------

    // The wallpaper of the current mode (the compositor draws it).
    readonly property string wallpaper: dark && (wallpaperSettings.dark ?? "").length
        ? wallpaperSettings.dark : (wallpaperSettings.light ?? "")
    readonly property WallpaperAccent wallpaperAccent: WallpaperAccent { source: theme.wallpaper }

    // --- Icons (CLAUDE.md 6.9, docs/formats/icon.md) --------------------

    // "auto": Default in light mode, Dark in dark mode.
    readonly property string iconMode: {
        const m = appearance.icon_mode ?? "auto"
        return m === "auto" ? (dark ? "dark" : "default") : m
    }
    readonly property color iconAccent: iconMode === "wallpaper" ? wallpaperAccent.color : accentBase
    // The image URL of an icon in the current mode: `id` an app id or a
    // name from the icon set, `legacy` the app's own icon file (or "").
    function icon(id, legacy) {
        return Icons.url(id, legacy ?? "", iconMode, iconAccent)
    }
    readonly property color selection: Qt.alpha(accent, dark ? 0.32 : 0.22)
    readonly property color focusRing: Qt.alpha(accent, 0.45)

    // --- Surfaces and text ----------------------------------------------

    readonly property color window: dark ? "#1A1F1C" : paper
    readonly property color sidebar: dark ? "#202622" : "#ECEFE9"
    readonly property color surface: dark ? "#232A25" : "#FFFFFF"
    readonly property color text: dark ? "#F2F4F0" : ink
    readonly property color secondaryText: dark ? "#AEB6B1" : "#5A625D"
    readonly property color disabledText: dark ? Qt.alpha("#F2F4F0", 0.32) : Qt.alpha(ink, 0.32)
    readonly property color separator: dark ? Qt.alpha("#FFFFFF", 0.10) : Qt.alpha(ink, 0.10)
    readonly property color fill: dark ? Qt.alpha("#FFFFFF", 0.09) : Qt.alpha(ink, 0.06)
    readonly property color fillHover: dark ? Qt.alpha("#FFFFFF", 0.13) : Qt.alpha(ink, 0.09)
    readonly property color fillPressed: dark ? Qt.alpha("#FFFFFF", 0.18) : Qt.alpha(ink, 0.13)
    readonly property color destructive: dark ? "#FF8A7A" : "#B3261E"
    readonly property color shadow: Qt.alpha("#000000", dark ? 0.5 : 0.22)

    // --- Materials (HIG: Materials) -------------------------------------
    // Each is a tint over the compositor's background blur, plus a hairline
    // edge; with reduce transparency the tint is opaque.

    function material(name) {
        const m = {
            menubar: dark ? ["#141815", 0.68] : [paper, 0.62],
            dock:    dark ? ["#1C221E", 0.52] : ["#FFFFFF", 0.42],
            popover: dark ? ["#1E2420", 0.80] : [paper, 0.80],
            menu:    dark ? ["#1E2420", 0.84] : [paper, 0.86],
            hud:     [ink, 0.78],
            sidebar: dark ? ["#1A1F1C", 0.70] : ["#ECEFE9", 0.72],
            sheet:   dark ? ["#1E2420", 0.92] : [paper, 0.94]
        }[name] ?? [window, 1.0]
        return Qt.alpha(m[0], reduceTransparency ? 1.0 : m[1])
    }
    readonly property color materialEdge: dark ? Qt.alpha("#FFFFFF", 0.12) : Qt.alpha("#FFFFFF", 0.55)
    readonly property color materialOutline: Qt.alpha("#000000", dark ? 0.45 : 0.10)

    // --- Type (HIG: Type) -----------------------------------------------

    readonly property string fontFamily: "Inter"
    readonly property string monoFamily: "JetBrains Mono"
    readonly property real textScale: ({ small: 0.92, "default": 1.0, large: 1.12, larger: 1.25 })[appearance.text_size ?? "default"] ?? 1.0
    function size(px) { return Math.round(px * textScale) }
    readonly property int largeTitle: size(26)
    readonly property int title1: size(22)
    readonly property int title2: size(17)
    readonly property int title3: size(15)
    readonly property int headline: size(13)
    readonly property int body: size(13)
    readonly property int callout: size(12)
    readonly property int subheadline: size(11)
    readonly property int footnote: size(10)

    // --- Space and shape ------------------------------------------------

    readonly property int unit: 4
    function space(n) { return n * unit }
    readonly property int radiusSmall: 5
    readonly property int radiusControl: 7
    readonly property int radiusCard: 10
    readonly property int radiusMenu: 9
    readonly property int radiusPopover: 14
    readonly property int radiusPanel: 18
    readonly property int controlHeight: 28
    readonly property int controlHeightLarge: 34

    // --- Motion (HIG: Motion) -------------------------------------------
    // Durations of the critically damped springs; with reduce motion,
    // movement is replaced by a short fade (callers check reduceMotion).

    readonly property int snappy: reduceMotion ? 0 : Motion.snappy
    readonly property int smooth: reduceMotion ? 0 : Motion.smooth
    readonly property int gentle: reduceMotion ? 0 : Motion.gentle
    readonly property int fade: 140
}
