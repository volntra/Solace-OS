import QtQuick
import Fern.Design

// A row of mutually exclusive choices. `model` is a list of labels;
// `currentIndex` the chosen one; `activated(index)` on a user's choice.
// The selection slides between segments on a spring.
FocusScope {
    id: root
    property var model: []
    property int currentIndex: 0
    signal activated(int index)
    implicitWidth: row.implicitWidth + 4
    implicitHeight: Theme.controlHeight
    activeFocusOnTab: true

    Keys.onLeftPressed: choose(Math.max(0, currentIndex - 1))
    Keys.onRightPressed: choose(Math.min(model.length - 1, currentIndex + 1))
    function choose(i) {
        if (i === currentIndex)
            return
        currentIndex = i
        activated(i)
    }

    Rectangle {
        anchors.fill: parent
        radius: Theme.radiusControl
        color: Theme.fill
        border.width: root.activeFocus ? 3 : 0
        border.color: Theme.focusRing
    }
    Rectangle {
        id: thumb
        readonly property Item target: row.children[root.currentIndex] ?? null
        x: target ? target.x + 2 : 2
        y: 2
        width: target ? target.width : 0
        height: parent.height - 4
        radius: Theme.radiusControl - 2
        color: Theme.dark ? Qt.alpha("#FFFFFF", 0.22) : "#FFFFFF"
        border.width: 1
        border.color: Qt.alpha("#000000", Theme.dark ? 0.2 : 0.06)
        Behavior on x { NumberAnimation { duration: Theme.snappy; easing: Motion.curve } }
        Behavior on width { NumberAnimation { duration: Theme.snappy; easing: Motion.curve } }
    }
    Row {
        id: row
        x: 2
        height: parent.height
        Repeater {
            model: root.model
            delegate: MouseArea {
                required property int index
                required property string modelData
                width: Math.max(56, label.implicitWidth + Theme.space(5))
                height: row.height
                onClicked: root.choose(index)
                Label {
                    id: label
                    anchors.centerIn: parent
                    text: parent.modelData
                    font.weight: parent.index === root.currentIndex ? Font.DemiBold : Font.Medium
                }
            }
        }
    }
}
