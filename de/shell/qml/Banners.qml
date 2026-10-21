import QtQuick
import Fern.Design
import Fern.Shell

// Notification banners: top right, newest on top, sliding in from the
// edge. The window is as tall as its banners, so the rest of the screen
// stays clickable. It stays mapped (1 px, clicks passing through) while
// there are none: a new surface's first frame can take long (a new GL
// context compiles its shaders), and a banner should appear at once.
ShellWindow {
    id: banners
    layer: ShellWindow.Overlay
    edges: ShellWindow.EdgeTop | ShellWindow.EdgeRight
    marginTop: 34
    marginRight: 6
    width: 360
    readonly property bool showing: Notifications.bannerCount > 0
    height: showing ? column.implicitHeight + 16 : 1
    scope: "fern-banners"
    open: true
    inputRect: showing ? Qt.rect(0, 0, width, height) : Qt.rect(0, 0, 1, 1)
    blurRect: column.children.length && showing
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
