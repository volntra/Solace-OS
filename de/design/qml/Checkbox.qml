import QtQuick
import QtQuick.Templates as T
import Fern.Design

T.CheckBox {
    id: control
    implicitWidth: indicator.width + (text.length ? spacing + implicitContentWidth : 0)
    implicitHeight: Math.max(18, implicitContentHeight)
    spacing: Theme.space(2)
    font.family: Theme.fontFamily
    font.pixelSize: Theme.body
    focusPolicy: Qt.StrongFocus

    indicator: Rectangle {
        anchors.verticalCenter: parent.verticalCenter
        width: 16
        height: 16
        radius: 4
        color: control.checked ? Theme.accent : Theme.surface
        border.width: control.checked ? 0 : 1
        border.color: Theme.dark ? Qt.alpha("#FFFFFF", 0.25) : Qt.alpha(Theme.ink, 0.25)
        opacity: control.enabled ? 1 : 0.45
        Symbol {
            anchors.centerIn: parent
            name: "check"
            size: 12
            color: Theme.textOnAccent
            visible: control.checked
        }
        Rectangle {
            anchors.fill: parent
            anchors.margins: -3
            radius: 6
            color: "transparent"
            border.width: control.visualFocus ? 3 : 0
            border.color: Theme.focusRing
        }
    }
    contentItem: Label {
        text: control.text
        enabled: control.enabled
        leftPadding: control.indicator.width + control.spacing
    }
}
