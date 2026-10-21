import QtQuick
import Fern.Design
import Fern.Shell

// The menu bar: the Fern menu, the active app's name and menus on the
// left (its own, exported over dbusmenu, or our fallback File and Window
// menus); apps' status icons, control center, search and the clock on the
// right.
ShellWindow {
    id: bar
    required property QtObject shell
    required property Popups popups
    readonly property int barHeight: 28
    layer: ShellWindow.Top
    edges: ShellWindow.EdgeTop | ShellWindow.EdgeLeft | ShellWindow.EdgeRight
    exclusiveZone: barHeight
    height: barHeight
    scope: "fern-menubar"
    open: true
    blurRect: shell.menubar.translucent ? Qt.rect(0, 0, width, height) : Qt.rect(0, 0, 0, 0)
    blurRadius: 0

    readonly property string appId: Apps.idForWindow(Windows.activeAppId)
    readonly property var app: Apps.get(appId)
    readonly property string appName: Windows.activeAppId.length ? Apps.displayName(Windows.activeAppId) : ""
    readonly property color fg: Theme.text

    Rectangle {
        anchors.fill: parent
        color: shell.menubar.translucent ? Theme.material("menubar") : Theme.window
        Rectangle {
            anchors.bottom: parent.bottom
            width: parent.width
            height: 1
            color: Theme.separator
        }
    }

    // A menu-bar item: text or a symbol, highlighted while its menu is open.
    component BarItem: MouseArea {
        id: item
        property string text
        property string symbol
        property bool bold: false
        property string menu
        readonly property bool active: menu.length > 0 && bar.popups.current === menu
        implicitWidth: (symbol.length ? 16 : label.implicitWidth) + 20
        height: bar.barHeight
        hoverEnabled: true
        onPressed: if (menu.length) bar.popups.toggle(menu, mapToItem(null, 0, 0).x)
        // Sliding along the bar with a menu open opens the next one.
        onContainsMouseChanged: if (containsMouse && menu.length && bar.popups.isMenu && !active)
            bar.popups.show(menu, mapToItem(null, 0, 0).x)
        Rectangle {
            anchors.fill: parent
            anchors.topMargin: 3
            anchors.bottomMargin: 3
            radius: Theme.radiusSmall
            color: item.active ? Theme.fillPressed : "transparent"
        }
        Label {
            id: label
            anchors.centerIn: parent
            visible: !item.symbol.length
            text: item.text
            font.weight: item.bold ? Font.Bold : Font.Medium
            color: bar.fg
        }
        Symbol {
            anchors.centerIn: parent
            visible: item.symbol.length > 0
            name: item.symbol
            size: 16
            color: bar.fg
        }
    }

    // The menus on the left, in order, with where each starts.
    function menuOrder() {
        const out = []
        for (const c of menus.children)
            if (c.visible && typeof c.menu === "string" && c.menu.length)
                out.push({ menu: c.menu, x: c.mapToItem(null, 0, 0).x })
        return out
    }

    Row {
        id: menus
        x: 8
        height: bar.barHeight
        BarItem { symbol: "fern-mark"; menu: "fern" }
        BarItem { text: bar.appName; bold: true; menu: "app"; visible: bar.appName.length > 0 }
        Repeater {
            model: bar.popups.appMenu.items
            BarItem {
                required property var modelData
                required property int index
                text: modelData.text
                menu: "appmenu:" + index
                visible: bar.appName.length > 0 && modelData.visible !== false
            }
        }
        BarItem { text: qsTr("File"); menu: "file"; visible: bar.appName.length > 0 && !bar.popups.appMenu.valid }
        BarItem { text: qsTr("Window"); menu: "window"; visible: bar.appName.length > 0 && !bar.popups.appMenu.valid }
    }

    // An app's status icon: its menu on a click, or, without one, the
    // app's own action.
    component TrayItem: MouseArea {
        id: tray
        required property string key
        required property string icon
        required property string title
        required property string service
        required property string menu
        readonly property bool hasMenu: menu.length > 0 && menu !== "/NO_DBUSMENU"
        readonly property bool active: bar.popups.current === "tray" && bar.popups.trayKey === key
        implicitWidth: 36
        height: bar.barHeight
        hoverEnabled: true
        onPressed: {
            const p = mapToItem(null, 0, 0)
            if (hasMenu) {
                if (active)
                    bar.popups.dismiss()
                else
                    bar.popups.showTray(key, service, menu, p.x)
            } else {
                const g = mapToGlobal(width / 2, height)
                Tray.activate(key, g.x, g.y)
            }
        }
        Rectangle {
            anchors.fill: parent
            anchors.topMargin: 3
            anchors.bottomMargin: 3
            radius: Theme.radiusSmall
            color: tray.active ? Theme.fillPressed : "transparent"
        }
        Image {
            anchors.centerIn: parent
            width: 16
            height: 16
            sourceSize: Qt.size(32, 32)
            source: tray.icon
            fillMode: Image.PreserveAspectFit
            smooth: true
            visible: tray.icon.length > 0
        }
        Label {
            anchors.centerIn: parent
            visible: tray.icon.length === 0
            text: tray.title.slice(0, 1).toUpperCase()
            font.weight: Font.Bold
            color: bar.fg
        }
    }

    Row {
        anchors.right: parent.right
        anchors.rightMargin: 8
        height: bar.barHeight
        Repeater {
            model: Tray
            TrayItem {}
        }
        BarItem {
            visible: Devices.wifiAvailable
            symbol: Devices.wifiEnabled ? "wifi" : "wifi-off"
            menu: "control"
        }
        BarItem {
            visible: Devices.soundAvailable
            symbol: Devices.muted || Devices.volume === 0 ? "volume-x" : Devices.volume < 0.5 ? "volume-1" : "volume-2"
            menu: "control"
        }
        BarItem { symbol: "sliders-horizontal"; menu: "control" }
        BarItem { symbol: "search"; menu: "search" }
        // Do Not Disturb: a moon by the clock, in the row so it takes room.
        BarItem {
            visible: Notifications.dnd
            symbol: "moon"
            menu: "notifications"
        }
        BarItem {
            id: clock
            menu: "notifications"
            property date now: new Date()
            text: {
                const s = bar.shell.menubar
                const time = Qt.formatTime(now, s.clock_24h ? (s.show_seconds ? "HH:mm:ss" : "HH:mm")
                                                         : (s.show_seconds ? "h:mm:ss AP" : "h:mm AP"))
                return s.show_date ? Qt.formatDate(now, "ddd d MMM") + "  " + time : time
            }
            Timer {
                interval: 1000; running: true; repeat: true
                onTriggered: clock.now = new Date()
            }
        }
    }
}
