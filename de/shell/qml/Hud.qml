import QtQuick
import Fern.Design
import Fern.Shell

// The volume HUD: a dark rounded square near the bottom of the screen
// with the level, after a volume key. Fades after a moment.
ShellWindow {
    id: hud
    layer: ShellWindow.Overlay
    edges: ShellWindow.EdgeBottom
    marginBottom: 140
    width: 180
    height: 180
    scope: "fern-hud"
    blurRect: Qt.rect(0, 0, width, height)
    blurRadius: Theme.radiusPanel
    inputRect: Qt.rect(0, 0, 1, 1)

    function show() {
        Devices.refresh()
        open = true
        hide.restart()
    }
    Timer { id: hide; interval: 1500; onTriggered: hud.open = false }

    Material {
        anchors.fill: parent
        kind: "hud"
        radius: Theme.radiusPanel
        Symbol {
            anchors.horizontalCenter: parent.horizontalCenter
            y: 40
            size: 64
            color: "#FFFFFF"
            name: !Devices.soundAvailable ? "volume-x"
                : Devices.muted || Devices.volume === 0 ? "volume-x" : Devices.volume < 0.5 ? "volume-1" : "volume-2"
        }
        // Sixteen steps, like the keys.
        Row {
            anchors.horizontalCenter: parent.horizontalCenter
            y: 140
            spacing: 2
            Repeater {
                model: 16
                Rectangle {
                    required property int index
                    width: 7
                    height: 6
                    radius: 1
                    color: !Devices.muted && Devices.volume * 16 >= index + 0.5 ? "#FFFFFF" : Qt.alpha("#FFFFFF", 0.22)
                }
            }
        }
        Label {
            visible: !Devices.soundAvailable
            anchors.horizontalCenter: parent.horizontalCenter
            y: 136
            text: qsTr("No sound output")
            color: "#FFFFFF"
            style: "callout"
        }
    }
}
