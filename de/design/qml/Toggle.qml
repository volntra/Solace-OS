import QtQuick
import QtQuick.Templates as T
import Fern.Design

// On/off switch. The knob springs across; the track takes the accent.
T.Switch {
    id: control
    implicitWidth: indicator.width + (text.length ? spacing + implicitContentWidth : 0)
    implicitHeight: Math.max(22, implicitContentHeight)
    spacing: Theme.space(2)
    font.family: Theme.fontFamily
    font.pixelSize: Theme.body
    focusPolicy: Qt.StrongFocus

    indicator: Rectangle {
        x: control.text.length ? control.width - width : 0
        anchors.verticalCenter: parent.verticalCenter
        width: 38
        height: 22
        radius: 11
        color: control.checked ? Theme.accent : Theme.fillPressed
        opacity: control.enabled ? 1 : 0.45
        border.width: control.visualFocus ? 3 : 0
        border.color: Theme.focusRing
        Behavior on color { ColorAnimation { duration: Theme.snappy; easing: Motion.curve } }
        Rectangle {
            id: knob
            width: 18
            height: 18
            radius: 9
            y: 2
            x: control.checked ? parent.width - width - 2 : 2
            color: "#FFFFFF"
            border.width: 1
            border.color: Qt.alpha("#000000", 0.06)
            Behavior on x { NumberAnimation { duration: Theme.snappy; easing: Motion.curve } }
        }
    }
    contentItem: Label {
        text: control.text
        enabled: control.enabled
        leftPadding: 0
        rightPadding: control.indicator.width + control.spacing
    }
}
