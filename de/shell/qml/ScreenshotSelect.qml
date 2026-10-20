import QtQuick
import QtQuick.Window
import Fern.Design
import Fern.Shell

// Choosing a rectangle for a screenshot: the desktop dims, a drag draws
// the rectangle (its size beside it), release takes it, Escape or a click
// without a drag cancels. Covers everything, the menu bar included.
ShellWindow {
    id: select
    // The rectangle, in desktop coordinates.
    signal chosen(rect area)
    layer: ShellWindow.Overlay
    edges: ShellWindow.EdgeTop | ShellWindow.EdgeBottom | ShellWindow.EdgeLeft | ShellWindow.EdgeRight
    exclusiveZone: -1
    keyboard: ShellWindow.ExclusiveKeyboard
    scope: "fern-screenshot"

    function start() {
        area.dragging = false
        open = true
    }

    Item {
        id: root
        anchors.fill: parent
        focus: true
        Keys.onEscapePressed: select.open = false

        // The dim, around the rectangle (none while nothing is chosen).
        readonly property rect r: area.dragging ? area.rect : Qt.rect(0, 0, 0, 0)
        readonly property color dim: Qt.alpha("#000000", 0.28)
        Rectangle { x: 0; y: 0; width: parent.width; height: root.r.y; color: root.dim }
        Rectangle { x: 0; y: root.r.y + root.r.height; width: parent.width; height: parent.height - y; color: root.dim }
        Rectangle { x: 0; y: root.r.y; width: root.r.x; height: root.r.height; color: root.dim }
        Rectangle { x: root.r.x + root.r.width; y: root.r.y; width: parent.width - x; height: root.r.height; color: root.dim }
        Rectangle {
            visible: area.dragging
            x: root.r.x - 1
            y: root.r.y - 1
            width: root.r.width + 2
            height: root.r.height + 2
            color: "transparent"
            border.width: 1
            border.color: "#FFFFFF"
        }
        // The size, by the pointer.
        Rectangle {
            visible: area.dragging
            x: Math.min(area.mouseX + 12, parent.width - width - 4)
            y: Math.min(area.mouseY + 12, parent.height - height - 4)
            width: sizeLabel.implicitWidth + 12
            height: sizeLabel.implicitHeight + 6
            radius: Theme.radiusSmall
            color: Qt.alpha(Theme.ink, 0.72)
            Label {
                id: sizeLabel
                anchors.centerIn: parent
                text: Math.round(root.r.width) + " × " + Math.round(root.r.height)
                color: "#FFFFFF"
                style: "callout"
            }
        }

        MouseArea {
            id: area
            anchors.fill: parent
            cursorShape: Qt.CrossCursor
            hoverEnabled: true
            property point from
            property bool dragging: false
            readonly property rect rect: Qt.rect(Math.min(from.x, mouseX), Math.min(from.y, mouseY),
                                                 Math.abs(mouseX - from.x), Math.abs(mouseY - from.y))
            onPressed: mouse => {
                from = Qt.point(mouse.x, mouse.y)
                dragging = true
            }
            onReleased: {
                const r = rect
                dragging = false
                select.open = false
                // A click is not a rectangle.
                if (r.width >= 4 && r.height >= 4)
                    select.chosen(Qt.rect(r.x + area.Screen.virtualX, r.y + area.Screen.virtualY, r.width, r.height))
            }
        }
    }
}
