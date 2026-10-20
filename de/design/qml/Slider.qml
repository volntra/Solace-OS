import QtQuick
import QtQuick.Templates as T
import Fern.Design

// A horizontal slider: a thin track filled with the accent up to a round
// knob. Arrow keys step by `stepSize` (default 1/20 of the range).
T.Slider {
    id: control
    implicitWidth: 200
    implicitHeight: 22
    stepSize: (to - from) / 20
    focusPolicy: Qt.StrongFocus
    background: Rectangle {
        x: control.leftPadding
        y: control.topPadding + control.availableHeight / 2 - height / 2
        width: control.availableWidth
        height: 4
        radius: 2
        color: Theme.fillPressed
        Rectangle {
            width: control.visualPosition * parent.width
            height: parent.height
            radius: 2
            color: control.enabled ? Theme.accent : Theme.disabledText
        }
    }
    handle: Rectangle {
        x: control.leftPadding + control.visualPosition * (control.availableWidth - width)
        y: control.topPadding + control.availableHeight / 2 - height / 2
        width: 18
        height: 18
        radius: 9
        color: "#FFFFFF"
        border.width: control.visualFocus ? 3 : 1
        border.color: control.visualFocus ? Theme.focusRing : Qt.alpha("#000000", 0.12)
    }
}
