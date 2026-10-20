import QtQuick
import Fern.Design
import Fern.Shell

// The control center: Wi-Fi, Bluetooth, Do Not Disturb, light/dark, Night
// Shift, the sound and brightness sliders, and what is playing.
Material {
    id: cc
    required property QtObject shell
    kind: "popover"
    radius: Theme.radiusPanel
    width: 324
    height: column.implicitHeight + 24
    Component.onCompleted: Devices.refresh()

    Column {
        id: column
        x: 12
        y: 12
        width: parent.width - 24
        spacing: 10

        Grid {
            columns: 2
            spacing: 8
            ControlTile {
                width: (column.width - 8) / 2
                symbol: Devices.wifiEnabled ? "wifi" : "wifi-off"
                title: qsTr("Wi-Fi")
                available: Devices.wifiAvailable
                on: Devices.wifiEnabled
                status: !available ? qsTr("Not available") : !on ? qsTr("Off") : Devices.network.length ? Devices.network : qsTr("Not connected")
                onToggled: Devices.wifiEnabled = !Devices.wifiEnabled
            }
            ControlTile {
                width: (column.width - 8) / 2
                symbol: Devices.bluetoothEnabled ? "bluetooth" : "bluetooth-off"
                title: qsTr("Bluetooth")
                available: Devices.bluetoothAvailable
                on: Devices.bluetoothEnabled
                status: !available ? qsTr("Not available") : on ? qsTr("On") : qsTr("Off")
                onToggled: Devices.bluetoothEnabled = !Devices.bluetoothEnabled
            }
            ControlTile {
                width: (column.width - 8) / 2
                symbol: "moon"
                title: qsTr("Do Not Disturb")
                on: cc.shell.notifications.dnd === true
                status: on ? qsTr("On") : qsTr("Off")
                onToggled: cc.shell.notifications.dnd = !on
            }
            ControlTile {
                width: (column.width - 8) / 2
                symbol: Theme.dark ? "moon-star" : "sun"
                title: Theme.dark ? qsTr("Dark") : qsTr("Light")
                on: Theme.dark
                status: Theme.mode === "auto" ? qsTr("Automatic") : qsTr("Appearance")
                onToggled: Theme.appearance.theme = Theme.dark ? "light" : "dark"
            }
            ControlTile {
                width: (column.width - 8) / 2
                symbol: "sun-moon"
                title: qsTr("Night Shift")
                on: cc.shell.nightShiftOn
                status: cc.shell.nightshift.manual ? qsTr("On") : on ? qsTr("Scheduled") : qsTr("Off")
                onToggled: cc.shell.toggleNightShift()
            }
        }

        // A labelled slider row with a symbol.
        component SliderRow: Column {
            id: row
            property alias value: slider.value
            property string title
            property string symbol
            property bool available: true
            property string missing
            signal moved(real v)
            width: column.width
            spacing: 4
            Label { text: row.title; style: "headline" }
            Row {
                spacing: 8
                visible: row.available
                Symbol { name: row.symbol; size: 16; color: Theme.secondaryText; anchors.verticalCenter: parent.verticalCenter }
                Slider {
                    id: slider
                    width: column.width - 24
                    from: 0
                    to: 1
                    stepSize: 1 / 16
                    onMoved: row.moved(value)
                }
            }
            Label { visible: !row.available; text: row.missing; style: "subheadline"; secondary: true }
        }
        SliderRow {
            title: qsTr("Sound")
            symbol: Devices.muted ? "volume-x" : "volume-2"
            available: Devices.soundAvailable
            missing: qsTr("No sound output")
            value: Devices.muted ? 0 : Devices.volume
            onMoved: v => Devices.volume = v
        }
        SliderRow {
            visible: Devices.brightnessAvailable
            title: qsTr("Display")
            symbol: "sun-medium"
            value: Devices.brightness
            onMoved: v => Devices.brightness = v
        }

        // Now playing.
        Rectangle {
            visible: Media.available
            width: column.width
            height: 56
            radius: Theme.radiusCard
            color: Theme.dark ? Qt.alpha("#FFFFFF", 0.07) : Qt.alpha("#FFFFFF", 0.55)
            border.width: 1
            border.color: Theme.separator
            Column {
                x: 12
                anchors.verticalCenter: parent.verticalCenter
                width: parent.width - 120
                Label { text: Media.title.length ? Media.title : Media.player; style: "headline"; width: parent.width }
                Label { text: Media.artist; style: "subheadline"; secondary: true; width: parent.width; visible: text.length > 0 }
            }
            Row {
                anchors.right: parent.right
                anchors.rightMargin: 8
                anchors.verticalCenter: parent.verticalCenter
                spacing: 2
                component MediaButton: MouseArea {
                    id: button
                    property string symbol
                    width: 30; height: 30
                    Symbol { anchors.centerIn: parent; name: button.symbol; size: 16 }
                }
                MediaButton { symbol: "skip-back"; onClicked: Media.previous() }
                MediaButton { symbol: Media.playing ? "pause" : "play"; onClicked: Media.playPause() }
                MediaButton { symbol: "skip-forward"; onClicked: Media.next() }
            }
        }
    }
}
