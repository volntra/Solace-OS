import QtQuick
import Fern.Design
import Fern.Shell

// Notification banners: top right, newest on top, sliding in from the
// edge. The window stays mapped at one size, room for four banners: a new
// surface's first frame can take long (a new GL context compiles its
// shaders), and resizing a layer surface while the compositor is slow
// races its configures. Only the banners shown take clicks; the rest is
// transparent and lets them through.
ShellWindow {
    id: banners
    layer: ShellWindow.Overlay
    edges: ShellWindow.EdgeTop | ShellWindow.EdgeRight
    marginTop: 34
    marginRight: 6
    width: 360
    readonly property bool showing: Notifications.bannerCount > 0
    height: 4 * 132 + 16
    scope: "fern-banners"
    open: true
    inputRect: showing ? Qt.rect(0, 0, width, Math.min(height, column.implicitHeight + 16)) : Qt.rect(0, 0, 1, 1)
    blurRect: column.children.length && showing
        ? Qt.rect(column.x, column.y, column.width, column.height) : Qt.rect(0, 0, 0, 0)
    blurRadius: Theme.radiusCard + 4

    Column {
        id: column
        // More than fit wait in the history (the Notification Center).
        clip: true
        height: Math.min(implicitHeight, banners.height - 16)
        x: 10
        y: 8
        width: 340
        spacing: 8
        Repeater {
            model: Notifications
            delegate: NotificationCard {
                required property bool banner
                visible: banner
                height: visible ? implicitHeight : 0
                // In from the right edge.
                property real slide: 0
                transform: Translate { x: slide }
                Component.onCompleted: if (!Theme.reduceMotion) { slide = 360; slide = 0 }
                Behavior on slide { NumberAnimation { duration: Theme.smooth; easing: Motion.curve } }
            }
        }
    }
}
