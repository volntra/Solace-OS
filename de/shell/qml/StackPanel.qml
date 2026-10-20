import QtQuick
import Fern.Design
import Fern.Shell

// A dock stack: the newest files in Downloads, or what is in the Trash
// (with Empty Trash). Files open once there is a Files app to open them
// with (Phase 11).
Material {
    id: panel
    property string stack: "downloads"
    kind: "popover"
    radius: Theme.radiusPanel
    width: 320
    height: Math.min(440, 56 + Math.max(1, files.count) * 40 + (stack === "trash" ? 44 : 0))

    readonly property string folder: stack === "trash"
        ? System.home + "/.local/share/Trash/files"
        : System.home + "/Downloads"

    // Read when the panel opens (it is made anew each time).
    readonly property var entries: System.listFolder(folder)
    QtObject {
        id: files
        readonly property int count: panel.entries.length
    }

    Label {
        x: 16
        y: 14
        text: panel.stack === "trash" ? qsTr("Trash") : qsTr("Downloads")
        style: "title3"
    }
    Label {
        anchors.horizontalCenter: parent.horizontalCenter
        y: 60
        visible: files.count === 0
        text: panel.stack === "trash" ? qsTr("The Trash is empty") : qsTr("Nothing downloaded yet")
        secondary: true
    }
    ListView {
        x: 8
        y: 48
        width: parent.width - 16
        height: parent.height - 56 - (panel.stack === "trash" ? 44 : 0)
        clip: true
        model: panel.entries
        delegate: Item {
            required property var modelData
            readonly property string fileName: modelData.name
            readonly property real fileSize: modelData.size
            readonly property date fileModified: modelData.modified
            readonly property bool fileIsDir: modelData.dir
            width: ListView.view.width
            height: 40
            Symbol {
                x: 8
                anchors.verticalCenter: parent.verticalCenter
                name: parent.fileIsDir ? "folder" : "file"
                size: 20
                color: Theme.secondaryText
            }
            Column {
                x: 38
                anchors.verticalCenter: parent.verticalCenter
                width: parent.width - 46
                Label { text: fileName; width: parent.width }
                Label {
                    text: Qt.formatDateTime(fileModified, "d MMM, h:mm AP") + (fileIsDir ? "" : "  ·  " + panel.size(fileSize))
                    style: "subheadline"
                    secondary: true
                }
            }
        }
    }
    Button {
        visible: panel.stack === "trash"
        enabled: files.count > 0
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.margins: 12
        text: confirm ? qsTr("Empty Trash: Sure?") : qsTr("Empty Trash…")
        kind: confirm ? "primary" : "secondary"
        property bool confirm: false
        onClicked: {
            if (!confirm) {
                confirm = true
                return
            }
            System.emptyTrash()
            confirm = false
        }
    }
    function size(bytes) {
        if (bytes < 1000) return bytes + " bytes"
        if (bytes < 1e6) return (bytes / 1e3).toFixed(0) + " KB"
        if (bytes < 1e9) return (bytes / 1e6).toFixed(1) + " MB"
        return (bytes / 1e9).toFixed(2) + " GB"
    }
}
