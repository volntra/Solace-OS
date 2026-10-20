import QtQuick
import Fern.Design

Item {
    implicitWidth: 100
    implicitHeight: 9
    Rectangle {
        anchors.verticalCenter: parent.verticalCenter
        x: 12
        width: parent.width - 24
        height: 1
        color: Theme.separator
    }
}
