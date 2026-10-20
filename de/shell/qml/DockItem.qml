import QtQuick
import Fern.Design
import Fern.Shell

// One dock tile: an app (`appId`) or a stack ("downloads", "trash").
Item {
    id: item
    // Not `dock`: in the dock, `dock: dock` would read this very property.
    required property Dock owner
    property string appId: ""
    property string stack: ""
    property bool pinned: false
    property int order: 0

    readonly property var app: appId.length ? Apps.get(appId) : ({})
    readonly property string label: stack === "downloads" ? qsTr("Downloads") : stack === "trash" ? qsTr("Trash") : Apps.displayName(appId)
    // The app id its windows carry (it may differ from the entry's id).
    readonly property string matchingWindowId: {
        for (const w of Windows.appIds)
            if (Apps.idForWindow(w) === appId)
                return w
        return appId
    }
    readonly property bool running: appId.length > 0 && Windows.appIds.includes(matchingWindowId)

    // Magnification: a smooth bump under the pointer, three icons wide.
    readonly property real centre: (owner.vertical ? y + height / 2 : x + width / 2) + owner.pad
    readonly property real scale0: {
        if (owner.pointer < 0)
            return 1
        const d = Math.abs(owner.pointer - centre) / (owner.iconSize * 1.6)
        const k = d >= 1 ? 0 : (Math.cos(d * Math.PI) + 1) / 2
        return 1 + (owner.bigSize / owner.iconSize - 1) * k
    }
    property real zoom: scale0
    Behavior on zoom { NumberAnimation { duration: Theme.snappy; easing: Motion.curve } }

    width: owner.vertical ? owner.thickness : owner.iconSize * zoom
    height: owner.vertical ? owner.iconSize * zoom : owner.thickness

    // What the app asks its icon to show (LauncherEntry): a count, a
    // progress bar, urgency (bounces until the app is brought forward).
    property var badge: Badges.get(appId)
    Connections {
        target: Badges
        function onChanged(id) { if (id === item.appId) item.badge = Badges.get(item.appId) }
    }

    // Launching: bounce until a window shows (at most 10 s).
    property bool launching: false
    onRunningChanged: {
        if (running)
            launching = false
        updateIconRects()
    }
    Timer { running: item.launching; interval: 10000; onTriggered: item.launching = false }
    property real bounce: 0
    SequentialAnimation on bounce {
        running: (item.launching || (item.badge.urgent && Windows.activeAppId !== item.matchingWindowId)) && !Theme.reduceMotion
        loops: Animation.Infinite
        NumberAnimation { from: 0; to: 1; duration: 300; easing.type: Easing.OutQuad }
        NumberAnimation { from: 1; to: 0; duration: 300; easing.type: Easing.InQuad }
        onRunningChanged: if (!running) item.bounce = 0
    }

    AppIcon {
        id: icon
        size: owner.iconSize * item.zoom
        appId: item.appId
        stack: item.stack
        name: item.label
        source: item.app.icon ?? ""
        // Grows away from the screen edge.
        x: owner.vertical ? (owner.position === "left" ? owner.pad : parent.width - owner.pad - size) - (owner.position === "left" ? -1 : 1) * item.bounce * 18 : 0
        y: owner.vertical ? 0 : parent.height - owner.pad - size - item.bounce * 18
    }

    // The count, top right of the icon.
    Rectangle {
        visible: item.badge.count > 0
        readonly property string label: item.badge.count > 99 ? "99+" : String(item.badge.count)
        x: icon.x + icon.size - width * 0.75
        y: icon.y - height * 0.25
        height: Math.max(18, icon.size * 0.36)
        width: Math.max(height, countText.implicitWidth + 10)
        radius: height / 2
        color: "#E5484D"
        border.width: 1.5
        border.color: Qt.alpha("#FFFFFF", 0.9)
        Text {
            id: countText
            anchors.centerIn: parent
            text: parent.label
            color: "#FFFFFF"
            font.family: Theme.fontFamily
            font.pixelSize: parent.height * 0.62
            font.weight: Font.Bold
        }
    }
    // Progress, along the bottom of the icon.
    Rectangle {
        visible: item.badge.progress >= 0
        x: icon.x + icon.size * 0.12
        y: icon.y + icon.size * 0.8
        width: icon.size * 0.76
        height: Math.max(5, icon.size * 0.1)
        radius: height / 2
        color: Qt.alpha("#000000", 0.45)
        Rectangle {
            width: parent.width * Math.max(0, item.badge.progress)
            height: parent.height
            radius: parent.radius
            color: "#FFFFFF"
        }
    }

    // Running: a dot on the edge side.
    Rectangle {
        visible: item.running
        width: 4; height: 4; radius: 2
        color: Theme.text
        opacity: 0.75
        x: owner.vertical ? (owner.position === "left" ? 2 : parent.width - 6) : (parent.width - width) / 2
        y: owner.vertical ? (parent.height - height) / 2 : parent.height - 5
    }

    // The name, over the icon while hovered.
    Rectangle {
        visible: mouse.containsMouse && !owner.popups.ownerIsDock
        width: tip.implicitWidth + 16
        height: 24
        radius: 8
        color: Theme.material("menu")
        border.width: 1
        border.color: Theme.materialOutline
        x: owner.vertical ? (owner.position === "left" ? icon.x + icon.size + 10 : icon.x - width - 10) : (parent.width - width) / 2
        y: owner.vertical ? (parent.height - height) / 2 : icon.y - height - 8
        Label { id: tip; anchors.centerIn: parent; text: item.label }
    }

    MouseArea {
        id: mouse
        anchors.fill: parent
        hoverEnabled: true
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        drag.target: item.pinned ? dragProxy : null
        onClicked: event => {
            if (event.button === Qt.RightButton) {
                owner.popups.openDockMenu(item, mapToItem(null, 0, 0))
                return
            }
            if (item.stack.length) {
                owner.popups.openStack(item.stack, mapToItem(null, width / 2, 0))
            } else if (item.running) {
                Windows.activateApp(item.matchingWindowId)
            } else if (Apps.launch(item.appId)) {
                item.launching = true
            }
        }
        onReleased: if (drag.active) item.dropAt(dragProxy)
    }
    // Reordering pinned apps: drag one along the bar.
    Item { id: dragProxy; width: 1; height: 1 }
    function dropAt(p) {
        owner.dragging = false
        const along = owner.vertical ? item.y + p.y : item.x + p.x
        const list = owner.pinned.slice()
        const from = list.indexOf(appId)
        if (from < 0)
            return
        const step = owner.iconSize + owner.gap
        const to = Math.max(0, Math.min(list.length - 1, Math.round(along / step)))
        list.splice(from, 1)
        list.splice(to, 0, appId)
        owner.setPinned(list)
        dragProxy.x = 0
        dragProxy.y = 0
    }

    // Minimize goes into this icon: tell the compositor where it is.
    function updateIconRects() {
        if (!running)
            return
        const p = icon.mapToItem(null, 0, 0)
        for (const uid of Windows.windowsOf(matchingWindowId))
            Windows.setIconRect(uid, owner, Qt.rect(p.x, p.y, icon.size, icon.size))
    }
    onXChanged: updateIconRects()
    onYChanged: updateIconRects()
    Connections {
        target: Windows
        function onCountChanged() { item.updateIconRects() }
    }
}
