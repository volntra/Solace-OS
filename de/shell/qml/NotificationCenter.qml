import QtQuick
import Fern.Design
import Fern.Shell

// The notification history (click the clock): newest first, each
// dismissable, Clear All at the top.
Material {
    id: center
    kind: "popover"
    radius: Theme.radiusPanel
    width: 364

    Label {
        x: 16
        y: 14
        text: qsTr("Notifications")
        style: "title3"
    }
    Button {
        anchors.right: parent.right
        anchors.rightMargin: 12
        y: 10
        kind: "plain"
        text: qsTr("Clear All")
        visible: Notifications.count > 0
        onClicked: Notifications.clearAll()
    }
    Label {
        anchors.centerIn: parent
        visible: Notifications.count === 0
        text: Notifications.dnd ? qsTr("Do Not Disturb is on.\nNew notifications wait here.") : qsTr("No notifications")
        horizontalAlignment: Text.AlignHCenter
        secondary: true
    }
    ListView {
        x: 12
        y: 48
        width: parent.width - 24
        height: parent.height - 60
        spacing: 8
        clip: true
        model: Notifications
        delegate: NotificationCard {
            width: 340
            color: Theme.dark ? Qt.alpha("#FFFFFF", 0.07) : Qt.alpha("#FFFFFF", 0.6)
        }
    }
}
