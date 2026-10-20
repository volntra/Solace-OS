import QtQuick
import Fern.Design

// A symbol (assets/symbols) in a colour, crisp at any size.
Image {
    property string name
    property color color: Theme.text
    property int size: 16
    width: size
    height: size
    sourceSize: Qt.size(size * Screen.devicePixelRatio, size * Screen.devicePixelRatio)
    source: name.length ? "image://symbol/" + name + "?color=" + encodeURIComponent(color.toString()) : ""
    smooth: true
    asynchronous: false
}
