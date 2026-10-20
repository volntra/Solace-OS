import QtQuick
import Fern.Design
import Fern.Shell

// The volume and brightness HUD: a dark rounded square near the bottom of
// the screen with the level, after a volume or brightness key. Fades
// after a moment.
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

    // "volume" or "brightness".
    property string kind: "volume"
    readonly property bool isVolume: kind === "volume"
    readonly property real level: isVolume ? (Devices.muted ? 0 : Devices.volume) : Devices.brightness
    readonly property bool unavailable: isVolume ? !Devices.soundAvailable : !Devices.brightnessAvailable

    function show(what) {
        kind = what ?? "volume"
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
            name: !hud.isVolume ? (hud.level < 0.5 ? "sun-medium" : "sun")
                : hud.unavailable || hud.level === 0 ? "volume-x" : hud.level < 0.5 ? "volume-1" : "volume-2"
        }
        // Sixteen steps, like the keys.
        Row {
            visible: !hud.unavailable
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
                    color: hud.level * 16 >= index + 0.5 ? "#FFFFFF" : Qt.alpha("#FFFFFF", 0.22)
                }
            }
        }
        Label {
            visible: hud.unavailable
            anchors.horizontalCenter: parent.horizontalCenter
            y: 136
            text: hud.isVolume ? qsTr("No sound output") : qsTr("No adjustable display")
            color: "#FFFFFF"
            style: "callout"
        }
    }
}
