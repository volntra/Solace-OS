import QtQuick
import Fern.Design
import Fern.Shell

// Notification banners: top right, newest on top, sliding in from the
// edge. The window is as tall as its banners, so the rest of the screen
// stays clickable.
ShellWindow {
    id: banners
    layer: ShellWindow.Overlay
    edges: ShellWindow.EdgeTop | ShellWindow.EdgeRight
    marginTop: 34
    marginRight: 6
    width: 360
    height: Math.max(1, column.implicitHeight + 16)
    scope: "fern-banners"
    open: Notifications.bannerCount > 0
    blurRect: column.children.length && Notifications.bannerCount > 0
        ? Qt.rect(column.x, column.y, column.width, column.implicitHeight) : Qt.rect(0, 0, 0, 0)
    blurRadius: Theme.radiusCard + 4

    Column {
        id: column
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
