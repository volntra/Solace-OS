import QtQuick
import QtQuick.Window
import Fern.Design

// An app's icon at `size`, drawn by the adaptive icon engine in the
// current icon mode (Theme.iconMode): the app's .fernicon, else its own
// icon file on a plate, else the generic app icon. A new mode, accent or
// wallpaper crossfades to the new look. Stacks (the Dock's Downloads and
// Trash) use the set's folder and Trash icons. If the engine can draw
// nothing at all (no icon set installed), a lettered tile stands in.
Item {
    id: icon
    property int size: 48
    property string appId: ""
    property string stack: ""
    property string name: ""
    property url source: ""
    width: size
    height: size

    readonly property string iconId: stack === "downloads" ? "folder.downloads" : stack === "trash" ? "trash" : appId
    readonly property string wanted: Theme.icon(iconId, stack.length ? "" : source.toString())
    // Pixels at the screen's scale, in steps, so a magnifying dock does not
    // ask for a new picture every frame.
    readonly property int pixels: {
        const px = size * Screen.devicePixelRatio
        return px <= 32 ? 32 : px <= 64 ? 64 : px <= 128 ? 128 : 256
    }

    // Two layers: the one shown, and the next look loading behind it.
    property Image front: first
    onWantedChanged: {
        const back = front === first ? second : first
        if (back.source.toString() === wanted && back.status === Image.Ready)
            front = back
        else
            back.source = wanted
    }
    Component.onCompleted: first.source = wanted

    component Layer: Image {
        anchors.fill: parent
        sourceSize: Qt.size(icon.pixels, icon.pixels)
        smooth: true
        mipmap: true
        opacity: icon.front === this ? 1 : 0
        visible: opacity > 0
        Behavior on opacity {
            NumberAnimation { duration: Theme.reduceMotion ? 0 : 450; easing.type: Easing.InOutQuad }
        }
        onStatusChanged: if (status === Image.Ready && source.toString() === icon.wanted) icon.front = this
    }
    Layer { id: first }
    Layer { id: second }

    Rectangle {
        visible: icon.front.status === Image.Error
        anchors.fill: parent
        anchors.margins: icon.size * 0.06
        radius: width * 0.225
        color: "#7A8580"
        Text {
            anchors.centerIn: parent
            text: icon.name.length ? icon.name.charAt(0).toUpperCase() : "?"
            font.family: Theme.fontFamily
            font.pixelSize: icon.size * 0.46
            font.weight: Font.Bold
            color: "#FFFFFF"
        }
    }
}
