import QtQuick
import Fern.Design
import Fern.Shell

// Menus and panels: one transparent layer over the desktop (below the
// menu bar, above the dock's room) that shows one menu or panel at a time
// and closes it on a click elsewhere or Escape. While open it has the
// keyboard.
ShellWindow {
    id: popups
    required property QtObject shell
    // The menu bar, for Left and Right between its menus.
    property MenuBar bar

    // "", a menu ("fern", "app", "file", "window", "appmenu:N" for the
    // active app's own menu N, "tray", "dock"), or a panel ("control",
    // "notifications", "search", "stack").
    property string current: ""
    property real anchorX: 0
    property point anchorPoint: Qt.point(0, 0)
    property Item dockItem: null
    property string stackName: ""
    readonly property bool isMenu: ["fern", "app", "file", "window", "tray"].includes(current) || current.startsWith("appmenu:")
    // The status icon whose menu is open.
    property string trayKey: ""

    // The active window's own menus, where it says they are; and the open
    // status icon's menu.
    readonly property DBusMenu appMenu: DBusMenu {
        service: Compositor.appMenuService
        path: Compositor.appMenuPath
    }
    readonly property DBusMenu trayMenu: DBusMenu {}
    readonly property bool ownerIsDock: current === "dock" || current === "stack"

    layer: ShellWindow.Overlay
    edges: ShellWindow.EdgeTop | ShellWindow.EdgeBottom | ShellWindow.EdgeLeft | ShellWindow.EdgeRight
    exclusiveZone: 0
    keyboard: ShellWindow.ExclusiveKeyboard
    scope: "fern-popups"
    open: current.length > 0
    blurRect: panel.item ? Qt.rect(panel.x + panel.item.x, panel.y + panel.item.y, panel.item.width, panel.item.height) : Qt.rect(0, 0, 0, 0)
    blurRadius: panel.item ? panel.item.radius ?? Theme.radiusPopover : 0

    function show(name, x) {
        anchorX = x ?? 0
        if (name.startsWith("appmenu:")) {
            const top = appMenu.items[Number(name.slice(8))]
            if (top)
                appMenu.aboutToShow(top.id)
        }
        current = name
    }
    // The menu bar's menu before (-1) or after (1) the open one.
    function step(d) {
        if (!isMenu || !bar)
            return
        const order = bar.menuOrder()
        const i = order.findIndex(m => m.menu === current)
        if (i < 0)
            return
        const next = order[(i + d + order.length) % order.length]
        show(next.menu, next.x)
    }
    function showTray(key, service, path, x) {
        trayKey = key
        trayMenu.service = service
        trayMenu.path = path
        trayMenu.aboutToShow(0)
        show("tray", x)
    }
    // A dbusmenu's entries as MenuPanel items.
    function menuItems(source, nodes) {
        return (nodes ?? []).map(n => n.separator ? { separator: true } : {
            text: n.text,
            shortcut: n.shortcut,
            enabled: n.enabled,
            checked: n.checked,
            children: menuItems(source, n.children),
            opened: () => source.aboutToShow(n.id),
            act: () => source.activate(n.id)
        })
    }
    function toggle(name, x) {
        if (current === name)
            dismiss()
        else
            show(name, x)
    }
    function dismiss() { current = "" }
    function openDockMenu(item, p) {
        dockItem = item
        anchorPoint = p
        current = "dock"
    }
    function openStack(name, p) {
        stackName = name
        anchorPoint = p
        current = "stack"
    }

    // A click outside the panel closes it.
    MouseArea {
        anchors.fill: parent
        onPressed: popups.dismiss()
    }
    // Sized by its panel, which places itself.
    Loader {
        id: panel
        focus: true
        // Escape closes whatever is open (keys rise from the panel to here).
        Keys.onEscapePressed: popups.dismiss()
        Keys.onLeftPressed: popups.step(-1)
        Keys.onRightPressed: popups.step(1)
        sourceComponent: {
            switch (popups.current) {
            case "fern": case "app": case "file": case "window": case "dock": case "tray": return menu
            case "control": return control
            case "notifications": return center
            case "search": return search
            case "stack": return stackPanel
            default: return popups.current.startsWith("appmenu:") ? menu : null
            }
        }
    }

    // --- The menus ------------------------------------------------------

    readonly property string activeApp: Windows.activeAppId
    readonly property var activeWindows: Windows.windowsOf(activeApp)
    readonly property string appName: Apps.displayName(activeApp)

    function itemsFor(name) {
        if (name.startsWith("appmenu:"))
            return menuItems(appMenu, appMenu.items[Number(name.slice(8))]?.children)
        switch (name) {
        case "tray": return menuItems(trayMenu, trayMenu.items)
        case "fern": return [
            { text: qsTr("About Fern"), act: () => about.show() },
            { separator: true },
            { text: qsTr("Settings…"), enabled: Apps.get("org.fern.Settings").id !== undefined,
              act: () => Apps.launch("org.fern.Settings") },
            { separator: true },
            { text: qsTr("Sleep"), enabled: System.canSleep },
            { text: qsTr("Restart…"), enabled: System.canPower, act: () => System.restart() },
            { text: qsTr("Shut Down…"), enabled: System.canPower, act: () => System.shutDown() },
            { separator: true },
            { text: qsTr("Lock Screen"), shortcut: "Ctrl Super Q", enabled: System.canLock },
            { text: qsTr("Log Out"), enabled: System.canLock }
        ]
        case "app": return [
            { text: qsTr("About %1").arg(appName), enabled: false },
            { separator: true },
            { text: qsTr("Hide %1").arg(appName), shortcut: "Super H",
              act: () => activeWindows.forEach(uid => Windows.minimize(uid)) },
            { separator: true },
            { text: qsTr("Quit %1").arg(appName), shortcut: "Super Q", act: () => Windows.closeApp(activeApp) }
        ]
        case "file": return [
            { text: qsTr("Close Window"), shortcut: "Super W", enabled: activeWindows.length > 0,
              act: () => Windows.close(activeWindows[0]) }
        ]
        case "window": return [
            { text: qsTr("Minimize"), shortcut: "Super M", enabled: activeWindows.length > 0,
              act: () => Windows.minimize(activeWindows[0]) },
            { separator: true },
            { text: qsTr("Bring All to Front"), enabled: activeWindows.length > 0,
              act: () => activeWindows.slice().reverse().forEach(uid => Windows.activate(uid)) }
        ]
        case "dock": {
            const it = dockItem
            if (!it)
                return []
            if (it.stack.length)
                return [{ text: qsTr("Open"), act: () => openStack(it.stack, anchorPoint) }]
            const list = []
            if (it.running)
                for (const uid of Windows.windowsOf(it.matchingWindowId))
                    list.push({ text: titleOf(uid), act: () => Windows.activate(uid) })
            if (list.length)
                list.push({ separator: true })
            if (!it.running)
                list.push({ text: qsTr("Open"), act: () => { if (Apps.launch(it.appId)) it.launching = true } })
            list.push(it.pinned
                ? { text: qsTr("Remove from Dock"), act: () => shell.dock.pinned = shell.dock.pinned.filter(a => a !== it.appId) }
                : { text: qsTr("Keep in Dock"), act: () => shell.dock.pinned = shell.dock.pinned.concat([it.appId]) })
            if (it.running) {
                list.push({ separator: true })
                list.push({ text: qsTr("Quit"), act: () => Windows.closeApp(it.matchingWindowId) })
            }
            return list
        }
        }
        return []
    }
    function titleOf(uid) {
        for (let i = 0; i < Windows.count; i++) {
            const idx = Windows.index(i, 0)
            if (Windows.data(idx, Qt.UserRole + 1) === uid)
                return Windows.data(idx, Qt.DisplayRole) || qsTr("Untitled")
        }
        return qsTr("Untitled")
    }

    Component {
        id: menu
        MenuPanel {
            items: popups.itemsFor(popups.current)
            x: popups.current === "dock" ? Math.min(popups.width - width - 8, Math.max(8, popups.anchorPoint.x - width / 2))
                                         : Math.min(popups.width - width - 8, popups.anchorX)
            y: popups.current === "dock" ? Math.max(8, popups.anchorPoint.y - popups.y - height - 12) : 2
            onDone: popups.dismiss()
        }
    }
    Component {
        id: control
        ControlCenter {
            shell: popups.shell
            x: popups.width - width - 8
            y: 6
        }
    }
    Component {
        id: center
        NotificationCenter {
            x: popups.width - width - 8
            y: 6
            height: popups.height - 12
        }
    }
    Component {
        id: search
        Search {
            x: (popups.width - width) / 2
            y: Math.round(popups.height * 0.18)
            onDone: popups.dismiss()
            onAction: name => {
                if (name === "about")
                    about.show()
                else if (name === "dnd")
                    popups.shell.notifications.dnd = !Notifications.dnd
                popups.dismiss()
            }
        }
    }
    Component {
        id: stackPanel
        StackPanel {
            stack: popups.stackName
            x: Math.min(popups.width - width - 8, Math.max(8, popups.anchorPoint.x - width / 2))
            y: Math.max(8, popups.anchorPoint.y - popups.y - height - 12)
        }
    }

    readonly property AboutPanel about: AboutPanel {}
}
