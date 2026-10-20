import QtQuick
import Fern.Design
import Fern.Shell

// One notification: who sent it, when, the summary and body, its
// actions. A click opens it (the "default" action); the cross dismisses.
Material {
    id: card
    required property int notificationId
    required property string app
    required property string summary
    required property string body
    required property var actions
    required property date time
    property bool banner: false
    kind: "popover"
    radius: Theme.radiusCard + 4
    implicitWidth: 340
    implicitHeight: content.implicitHeight + 24
    width: implicitWidth
    height: implicitHeight

    readonly property var buttons: actions.filter(a => a.key !== "default")

    MouseArea {
        id: area
        anchors.fill: parent
        hoverEnabled: true
        onClicked: Notifications.invoke(card.notificationId, "default")
        onContainsMouseChanged: if (card.banner) Notifications.hold(card.notificationId, containsMouse)
    }
    Column {
        id: content
        x: 14
        y: 12
        width: parent.width - 28
        spacing: 3
        Row {
            width: parent.width
            spacing: 6
            AppIcon {
                size: 18
                appId: card.app
                name: card.app
                source: ""
            }
            Label {
                text: card.app.length ? card.app : qsTr("Notification")
                style: "subheadline"
                secondary: true
                width: parent.width - 100
            }
            Label {
                text: Qt.formatTime(card.time, "h:mm AP")
                style: "subheadline"
                secondary: true
            }
        }
        Label {
            text: card.summary
            style: "headline"
            width: parent.width
            wrapMode: Text.Wrap
            maximumLineCount: 2
        }
        Label {
            text: card.body
            visible: text.length > 0
            width: parent.width
            wrapMode: Text.Wrap
            maximumLineCount: card.banner ? 3 : 6
            textFormat: Text.StyledText
        }
        Row {
            visible: card.buttons.length > 0
            spacing: 6
            topPadding: 6
            Repeater {
                model: card.buttons
                Button {
                    required property var modelData
                    text: modelData.label
                    onClicked: Notifications.invoke(card.notificationId, modelData.key)
                }
            }
        }
    }
    // Dismiss, shown on hover.
    MouseArea {
        visible: area.containsMouse || containsMouse
        hoverEnabled: true
        x: -6
        y: -6
        width: 20
        height: 20
        onClicked: Notifications.dismiss(card.notificationId)
        Rectangle {
            anchors.fill: parent
            radius: 10
            color: Theme.material("menu")
            border.width: 1
            border.color: Theme.materialOutline
            Symbol { anchors.centerIn: parent; name: "x"; size: 12 }
        }
    }
}
