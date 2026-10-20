import QtQuick
import Fern.Design

// A control center tile: a round symbol button (the accent when on), a
// title and a line of status. Unavailable tiles say why and do nothing.
MouseArea {
    id: tile
    property string symbol
    property string title
    property string status
    property bool on: false
    property bool available: true
    signal toggled()
    implicitWidth: 148
    implicitHeight: 56
    enabled: available
    onClicked: toggled()

    Rectangle {
        anchors.fill: parent
        radius: Theme.radiusCard
        color: Theme.dark ? Qt.alpha("#FFFFFF", 0.07) : Qt.alpha("#FFFFFF", 0.55)
        border.width: 1
        border.color: Theme.separator
    }
    Rectangle {
        id: badge
        x: 10
        anchors.verticalCenter: parent.verticalCenter
        width: 30
        height: 30
        radius: 15
        color: tile.on && tile.available ? Theme.accent : Theme.fillPressed
        Symbol {
            anchors.centerIn: parent
            name: tile.symbol
            size: 16
            color: tile.on && tile.available ? Theme.textOnAccent : Theme.text
            opacity: tile.available ? 1 : 0.5
        }
    }
    Column {
        anchors.left: badge.right
        anchors.leftMargin: 8
        anchors.right: parent.right
        anchors.rightMargin: 8
        anchors.verticalCenter: parent.verticalCenter
        Label { text: tile.title; style: "headline"; width: parent.width; enabled: tile.available }
        Label { text: tile.status; style: "subheadline"; secondary: true; width: parent.width }
    }
}
