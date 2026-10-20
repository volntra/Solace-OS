import QtQuick
import Fern.Design

// A text field with the search symbol and a clear button.
TextField {
    id: control
    leftPadding: Theme.space(7)
    rightPadding: length ? Theme.space(7) : Theme.space(2.5)
    placeholderText: qsTr("Search")
    Symbol {
        x: Theme.space(2.5)
        anchors.verticalCenter: parent.verticalCenter
        name: "search"
        size: Math.round(control.height * 0.5)
        color: Theme.secondaryText
    }
    MouseArea {
        visible: control.length > 0
        width: control.height
        height: control.height
        anchors.right: parent.right
        cursorShape: Qt.ArrowCursor
        onClicked: control.clear()
        Symbol {
            anchors.centerIn: parent
            name: "x"
            size: Math.round(control.height * 0.45)
            color: Theme.secondaryText
        }
    }
}
