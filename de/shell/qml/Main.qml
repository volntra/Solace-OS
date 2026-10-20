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
    readonly property SettingsDomain wallpaper: SettingsDomain {
        domain: "wallpaper"
        defaults: ({ light: "/usr/share/fern/wallpapers/grove.png",
                     dark: "/usr/share/fern/wallpapers/grove-night.png" })
    }

    // Do Not Disturb and banner time live in settings; the server follows.
    readonly property Binding dndBinding: Binding {
        target: Notifications; property: "dnd"; value: shell.notifications.dnd
    }
    readonly property Binding bannerBinding: Binding {
        target: Notifications; property: "bannerSeconds"; value: shell.notifications.banner_seconds
    }

    // The wallpaper of the current mode, crossfaded by the compositor.
    readonly property string wallpaperPath: Theme.dark && (wallpaper.dark ?? "").length ? wallpaper.dark : (wallpaper.light ?? "")
    onWallpaperPathChanged: Compositor.setWallpaper(wallpaperPath)
    Component.onCompleted: {
        Compositor.setWallpaper(wallpaperPath)
        // Super+Space: Search. The volume keys.
        Compositor.bindKey(1, 8, 0x20)
        Compositor.bindKey(2, 0, 0x1008FF13)
        Compositor.bindKey(3, 0, 0x1008FF11)
        Compositor.bindKey(4, 0, 0x1008FF12)
    }
    readonly property Connections keys: Connections {
        target: Compositor
        function onKeyBinding(id) {
            if (id === 1)
                popups.toggle("search")
            else if (id >= 2 && id <= 4) {
                Devices.stepVolume(id === 2 ? 1 : id === 3 ? -1 : 0)
                hud.show()
            }
        }
    }

    readonly property Popups popups: Popups {
        shell: shell
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
