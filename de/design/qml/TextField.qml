import QtQuick
import QtQuick.Templates as T
import Fern.Design

T.TextField {
    id: control
    implicitWidth: 200
    implicitHeight: Theme.controlHeight
    leftPadding: Theme.space(2.5)
    rightPadding: Theme.space(2.5)
    font.family: Theme.fontFamily
    font.pixelSize: Theme.body
    color: Theme.text
    placeholderTextColor: Theme.secondaryText
    selectionColor: Theme.selection
    selectedTextColor: Theme.text
    verticalAlignment: TextInput.AlignVCenter

    Text {
        x: control.leftPadding
        width: control.width - control.leftPadding - control.rightPadding
        height: control.height
        verticalAlignment: Text.AlignVCenter
        text: control.placeholderText
        font: control.font
        color: control.placeholderTextColor
        visible: !control.length && !control.preeditText
        elide: Text.ElideRight
    }
    background: Rectangle {
        radius: Theme.radiusControl
        color: Theme.dark ? Qt.alpha("#FFFFFF", 0.07) : "#FFFFFF"
        border.width: control.activeFocus ? 3 : 1
        border.color: control.activeFocus ? Theme.focusRing : Theme.separator
    }
}
