import QtQuick
import Fern.Design

// A translucent surface: the material's tint over the compositor's blur
// (asked for by the window holding it), a light inner edge and a faint
// outline. The blur's own dither gives the grain. `kind` is menubar, dock,
// popover, menu, hud, sidebar or sheet.
Rectangle {
    property string kind: "popover"
    color: Theme.material(kind)
    radius: Theme.radiusPopover
    border.width: 1
    border.color: Theme.materialOutline
    Rectangle {
        anchors.fill: parent
        anchors.margins: 1
        radius: Math.max(0, parent.radius - 1)
        color: "transparent"
        border.width: 1
        border.color: Theme.materialEdge
        opacity: Theme.reduceTransparency ? 0 : 1
    }
}
