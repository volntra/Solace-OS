import QtQuick
import Fern.Design

// One row of a menu: text, an optional shortcut, highlighted with the
// accent while hovered or chosen with the keyboard. `triggered` on click.
// A row that opens a submenu shows a chevron instead of a shortcut.
MouseArea {
    id: item
    property string text
    property string shortcut: ""
    property bool highlighted: containsMouse
    property bool checked: false
    property bool submenu: false
    signal triggered()
    implicitWidth: label.implicitWidth + (submenu ? 16 : keys.implicitWidth) + Theme.space(14)
    implicitHeight: 24
    hoverEnabled: true
    onClicked: if (enabled) triggered()

    Rectangle {
        anchors.fill: parent
        anchors.leftMargin: 5
        anchors.rightMargin: 5
        radius: Theme.radiusSmall
        color: item.highlighted && item.enabled ? Theme.accent : "transparent"
    }
    Symbol {
        x: 10
        anchors.verticalCenter: parent.verticalCenter
        visible: item.checked
        name: "check"
        size: 12
        color: item.highlighted ? Theme.textOnAccent : Theme.text
    }
    Label {
        id: label
        x: 26
        anchors.verticalCenter: parent.verticalCenter
        text: item.text
        enabled: item.enabled
        color: !item.enabled ? Theme.disabledText : item.highlighted ? Theme.textOnAccent : Theme.text
    }
    Label {
        id: keys
        anchors.right: parent.right
        anchors.rightMargin: 14
        anchors.verticalCenter: parent.verticalCenter
        visible: !item.submenu
        text: item.shortcut
        color: item.highlighted && item.enabled ? Theme.textOnAccent : Theme.secondaryText
    }
    Symbol {
        anchors.right: parent.right
        anchors.rightMargin: 12
        anchors.verticalCenter: parent.verticalCenter
        visible: item.submenu
        name: "chevron-right"
        size: 12
        color: item.highlighted && item.enabled ? Theme.textOnAccent : Theme.secondaryText
    }
}
