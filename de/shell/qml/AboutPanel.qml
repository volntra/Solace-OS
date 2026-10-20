import QtQuick
import QtQuick.Window
import Fern.Design
import Fern.Shell

// About Fern: an ordinary window (with our titlebar) showing the logo,
// the version and this computer.
Window {
    id: about
    title: qsTr("About Fern")
    width: 420
    height: 300
    minimumWidth: 420
    minimumHeight: 300
    maximumWidth: 420
    maximumHeight: 300
    color: Theme.window
    visible: false

    Image {
        id: logo
        anchors.horizontalCenter: parent.horizontalCenter
        y: 26
        width: 88
        height: 88
        sourceSize: Qt.size(176, 176)
        source: "image://symbol/fern-mark?color=" + encodeURIComponent(Theme.dark ? Theme.leaf : Theme.fern)
    }
    Label {
        id: name
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.top: logo.bottom
        anchors.topMargin: 10
        text: "Fern"
        style: "largeTitle"
    }
    Label {
        id: version
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.top: name.bottom
        text: qsTr("Version %1").arg(System.version)
        secondary: true
    }
    Grid {
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.top: version.bottom
        anchors.topMargin: 18
        columns: 2
        columnSpacing: 12
        rowSpacing: 4
        horizontalItemAlignment: Grid.AlignLeft
        Label { text: qsTr("Name"); secondary: true; horizontalAlignment: Text.AlignRight; width: 90 }
        Label { text: System.hostName }
        Label { text: qsTr("Processor"); secondary: true; horizontalAlignment: Text.AlignRight; width: 90; visible: System.processor.length > 0 }
        Label { text: System.processor; visible: text.length > 0; width: 280 }
        Label { text: qsTr("Memory"); secondary: true; horizontalAlignment: Text.AlignRight; width: 90; visible: System.memory.length > 0 }
        Label { text: System.memory; visible: text.length > 0 }
        Label { text: qsTr("Graphics"); secondary: true; horizontalAlignment: Text.AlignRight; width: 90; visible: System.graphics.length > 0 }
        Label { text: System.graphics; visible: text.length > 0 }
    }
}
