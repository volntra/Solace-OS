import QtQuick
import Fern.Design
import Fern.Shell

// The shell: one menu bar, one dock, one layer for menus and panels, the
// notification banners and the volume HUD. Every setting it uses is live.
QtObject {
    id: shell

    readonly property SettingsDomain dock: SettingsDomain {
        domain: "dock"
        defaults: ({ position: "bottom", icon_size: 52, magnification: true, magnified_size: 76,
                     autohide: false, pinned: ["org.fern.Files", "org.fern.Terminal", "org.fern.Settings"],
                     show_recent: true })
    }
    readonly property SettingsDomain menubar: SettingsDomain {
        domain: "menubar"
        defaults: ({ translucent: true, clock_24h: false, show_date: true, show_seconds: false })
    }
    readonly property SettingsDomain notifications: SettingsDomain {
        domain: "notifications"
        defaults: ({ dnd: false, banner_seconds: 5 })
    }
    readonly property SettingsDomain nightshift: SettingsDomain {
        domain: "nightshift"
        defaults: ({ manual: false, schedule: "off", from: 1320, to: 420, warmth: 60 })
    }

    // Do Not Disturb and banner time live in settings; the server follows.
    readonly property Binding dndBinding: Binding {
        target: Notifications; property: "dnd"; value: shell.notifications.dnd
    }
    readonly property Binding bannerBinding: Binding {
        target: Notifications; property: "bannerSeconds"; value: shell.notifications.banner_seconds
    }

    // Night Shift: on by hand or by its schedule (checked every minute).
    property int minutes: NightShift.minutesNow()
    readonly property Timer minuteClock: Timer {
        interval: 30000; running: true; repeat: true
        onTriggered: shell.minutes = NightShift.minutesNow()
    }
    readonly property bool nightShiftScheduled:
        NightShift.scheduled(nightshift.schedule ?? "off", nightshift.from ?? 0, nightshift.to ?? 0, minutes)
    // Turned off by hand while the schedule had it on: off until the
    // schedule next changes (or the shell restarts).
    property bool nightShiftSkipped: false
    onNightShiftScheduledChanged: nightShiftSkipped = false
    readonly property bool nightShiftOn: nightshift.manual === true || (nightShiftScheduled && !nightShiftSkipped)
    function toggleNightShift() {
        if (nightShiftOn) {
            nightshift.manual = false
            nightShiftSkipped = nightShiftScheduled
        } else {
            nightshift.manual = true
        }
    }
    readonly property int colorTemperature: nightShiftOn ? NightShift.kelvin(nightshift.warmth ?? 60) : 0
    onColorTemperatureChanged: Compositor.setColorTemperature(colorTemperature)

    // The wallpaper of the current mode, crossfaded by the compositor.
    readonly property string wallpaperPath: Theme.wallpaper
    onWallpaperPathChanged: Compositor.setWallpaper(wallpaperPath)
    // Titlebars follow the appearance too.
    readonly property bool dark: Theme.dark
    onDarkChanged: Compositor.setDarkMode(dark)
    // And the app switcher's icons follow the dock's.
    readonly property string iconStyle: Theme.iconMode + Theme.iconAccent
    onIconStyleChanged: Compositor.setIconStyle(Theme.iconMode, Theme.iconAccent)
    Component.onCompleted: {
        Compositor.setWallpaper(wallpaperPath)
        Compositor.setColorTemperature(colorTemperature)
        Compositor.setDarkMode(Theme.dark)
        Compositor.setIconStyle(Theme.iconMode, Theme.iconAccent)
        // Super+Space: Search. The volume and brightness keys.
        Compositor.bindKey(1, 8, 0x20)
        Compositor.bindKey(2, 0, 0x1008FF13)
        Compositor.bindKey(3, 0, 0x1008FF11)
        Compositor.bindKey(4, 0, 0x1008FF12)
        Compositor.bindKey(5, 0, 0x1008FF02)
        Compositor.bindKey(6, 0, 0x1008FF03)
        // Shift+Super+3: a screenshot of the display; Shift+Super+4: of a
        // rectangle. Print: the display.
        Compositor.bindKey(7, 9, 0x33)
        Compositor.bindKey(8, 9, 0x34)
        Compositor.bindKey(9, 0, 0xff61)
    }
    readonly property Connections keys: Connections {
        target: Compositor
        function onKeyBinding(id) {
            if (id === 1)
                popups.toggle("search")
            else if (id >= 2 && id <= 4) {
                Devices.stepVolume(id === 2 ? 1 : id === 3 ? -1 : 0)
                hud.show("volume")
            } else if (id === 5 || id === 6) {
                Devices.stepBrightness(id === 5 ? 1 : -1)
                hud.show("brightness")
            } else if (id === 7 || id === 9) {
                shell.screenshot(Qt.rect(0, 0, 0, 0))
            } else if (id === 8) {
                popups.dismiss()
                screenshotSelect.start()
            }
        }
        function onScreenshotTaken(id, image) {
            Screenshots.saveNow(image)
        }
        function onScreenshotFailed(id) {
            Notifications.post(qsTr("Screenshot"), qsTr("The screenshot could not be taken"), "")
        }
    }

    // A screenshot of `area` (desktop coordinates; empty: the display).
    function screenshot(area) {
        if (Compositor.screenshot(area) < 0)
            Notifications.post(qsTr("Screenshot"), qsTr("The screenshot could not be taken"), "")
    }
    readonly property Connections screenshotFiles: Connections {
        target: Screenshots
        function onSaved(path) {
            Notifications.post(qsTr("Screenshot"), qsTr("Screenshot saved"), path.split("/").pop(), path)
        }
        function onFailed() {
            Notifications.post(qsTr("Screenshot"), qsTr("The screenshot could not be saved"), Screenshots.folder())
        }
    }
    readonly property ScreenshotSelect screenshotSelect: ScreenshotSelect {
        onChosen: area => Qt.callLater(() => shell.screenshot(area))
    }

    readonly property Popups popups: Popups {
        shell: shell
        bar: shell.bar
    }
    readonly property MenuBar bar: MenuBar {
        shell: shell
        popups: shell.popups
    }
    readonly property Dock dockWindow: Dock {
        shell: shell
        popups: shell.popups
    }
    readonly property Banners banners: Banners {}
    readonly property Hud hud: Hud {}
}
