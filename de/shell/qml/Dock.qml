import QtQuick
import Fern.Design
import Fern.Shell

// The dock: pinned apps, then running ones that are not pinned (and,
// with show_recent, the last few used), a divider, Downloads and Trash.
// Icons magnify under the pointer; running apps have a dot; a launching
// app bounces until its window appears.
ShellWindow {
    id: dock
    required property QtObject shell
    required property Popups popups
    readonly property QtObject settings: shell.dock

    readonly property string position: settings.position ?? "bottom"
    readonly property bool vertical: position !== "bottom"
    readonly property int iconSize: settings.icon_size ?? 52
    readonly property int bigSize: settings.magnification ? Math.max(iconSize, settings.magnified_size ?? 76) : iconSize
    readonly property int pad: Math.round(iconSize * 0.14)
    readonly property int gap: 6
    readonly property int edgeMargin: 6
    readonly property int thickness: iconSize + 2 * pad
    readonly property bool autohide: settings.autohide === true
    // Shown: always, or (autohide) while the pointer is near or a menu of
    // ours is open.
    readonly property bool revealed: !autohide || hover.containsMouse || dragging || popups.ownerIsDock
    property bool dragging: false

    layer: ShellWindow.Top
    edges: position === "bottom" ? ShellWindow.EdgeBottom : position === "left" ? ShellWindow.EdgeLeft : ShellWindow.EdgeRight
    exclusiveZone: autohide ? 0 : thickness + edgeMargin
    scope: "fern-dock"
    open: true
    // Room for magnified icons and their labels beyond the bar.
    readonly property int headroom: bigSize - iconSize + 40
    width: vertical ? thickness + edgeMargin + headroom : Math.max(bar.length + 2 * bigSize, 200)
    height: vertical ? Math.max(bar.length + 2 * bigSize, 200) : thickness + edgeMargin + headroom

    // Where the bar is, in window coordinates.
    readonly property rect barRect: {
        const len = bar.length
        if (!vertical)
            return Qt.rect((width - len) / 2, height - edgeMargin - thickness + slide, len, thickness)
        const x = position === "left" ? edgeMargin - slide : width - edgeMargin - thickness + slide
        return Qt.rect(x, (height - len) / 2, thickness, len)
    }
    blurRect: revealed ? barRect : Qt.rect(0, 0, 0, 0)
    blurRadius: Theme.radiusPanel
    // Clicks pass through the headroom (and, hidden, all but a strip at
    // the edge).
    inputRect: revealed ? (vertical ? Qt.rect(barRect.x - (position === "left" ? 0 : headroom), barRect.y, thickness + headroom, barRect.height)
                                    : Qt.rect(barRect.x, barRect.y - headroom, barRect.width, thickness + headroom))
                        : (vertical ? Qt.rect(position === "left" ? 0 : width - 3, 0, 3, height)
                                    : Qt.rect(0, height - 3, width, 3))
    property real slide: revealed ? 0 : thickness + edgeMargin + 4
    Behavior on slide { NumberAnimation { duration: Theme.smooth; easing: Motion.curve } }

    // --- What the dock holds --------------------------------------------

    readonly property var pinned: settings.pinned ?? []
    property var recent: []
    readonly property var running: {
        const out = []
        for (const w of Windows.appIds) {
            const id = Apps.idForWindow(w)
            if (id.length && !out.includes(id))
                out.push(id)
        }
        return out
    }
    readonly property var apps: {
        const list = []
        for (const id of pinned)
            if (Apps.get(id).id !== undefined || running.includes(id))
                list.push({ id: id, pinned: true })
        for (const id of running)
            if (!pinned.includes(id))
                list.push({ id: id, pinned: false })
        if (settings.show_recent)
            for (const id of recent)
                if (!pinned.includes(id) && !running.includes(id) && Apps.get(id).id !== undefined)
                    list.push({ id: id, pinned: false, recent: true })
        return list
    }
    onRunningChanged: {
        // Apps that quit become recent (the last three).
        const gone = previousRunning.filter(id => !running.includes(id))
        recent = gone.concat(recent.filter(id => !gone.includes(id) && !running.includes(id))).slice(0, 3)
        previousRunning = running
    }
    property var previousRunning: []

    function setPinned(list) { settings.pinned = list }

    // --- Drawing --------------------------------------------------------

    MouseArea {
        id: hover
        anchors.fill: parent
        hoverEnabled: true
        acceptedButtons: Qt.NoButton
    }

    Material {
        kind: "dock"
        x: dock.barRect.x
        y: dock.barRect.y
        width: dock.barRect.width
        height: dock.barRect.height
        radius: Theme.radiusPanel
        visible: dock.slide < dock.thickness
    }

    // The pointer along the bar, for magnification (-1: away).
    readonly property real pointer: hover.containsMouse && settings.magnification
        ? (vertical ? hover.mouseY - barRect.y : hover.mouseX - barRect.x) : -1

    Item {
        id: bar
        x: dock.barRect.x
        y: dock.barRect.y
        width: dock.barRect.width
        height: dock.barRect.height
        // Length along the bar: icons at their magnified size plus gaps.
        readonly property real length: content.length + 2 * dock.pad
        Flow {
            id: content
            readonly property real length: dock.vertical ? implicitHeight : implicitWidth
            x: dock.vertical ? 0 : dock.pad
            y: dock.vertical ? dock.pad : 0
            flow: dock.vertical ? Flow.TopToBottom : Flow.LeftToRight
            spacing: dock.gap
            width: dock.vertical ? dock.thickness : implicitWidth
            height: dock.vertical ? implicitHeight : dock.thickness
            Repeater {
                model: dock.apps
                delegate: DockItem {
                    required property var modelData
                    required property int index
                    dock: dock
                    appId: modelData.id
                    pinned: modelData.pinned
                    order: index
                }
            }
            Rectangle {
                width: dock.vertical ? dock.iconSize * 0.7 : 1
                height: dock.vertical ? 1 : dock.iconSize * 0.7
                anchors.verticalCenter: dock.vertical ? undefined : parent.verticalCenter
                x: dock.vertical ? (dock.thickness - width) / 2 : 0
                y: dock.vertical ? 0 : (dock.thickness - height) / 2
                color: Theme.separator
            }
            DockItem { dock: dock; stack: "downloads"; order: -1 }
            DockItem { dock: dock; stack: "trash"; order: -2 }
        }
    }
}
