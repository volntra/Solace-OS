import QtQuick
import Fern.Design

// An app's icon at `size`. Apps without one (and until the adaptive icon
// engine arrives in Phase 7) get a tile: our rounded square in a colour
// picked from the app's id, with its initial. Stacks get folder tiles.
Item {
    id: icon
    property int size: 48
    property string appId: ""
    property string stack: ""
    property string name: ""
    property url source: ""
    width: size
    height: size

    readonly property var tints: [
        ["#5EA8F7", "#2C6FD6"], ["#62CC8C", "#1C7546"], ["#FFB56B", "#EE6419"],
        ["#B990FF", "#6438DB"], ["#58D6C4", "#178A82"], ["#F07AA8", "#B83468"],
        ["#FFD27A", "#E59A1F"], ["#9AA39D", "#525A57"]
    ]
    readonly property var colors: {
        let h = 0
        for (let i = 0; i < appId.length; i++)
            h = (h * 31 + appId.charCodeAt(i)) >>> 0
        return stack === "trash" ? ["#F2F4F1", "#C9CFCA"] : stack === "downloads" ? ["#8DB8F8", "#3F7EE8"] : tints[h % tints.length]
    }

    Image {
        id: picture
        anchors.fill: parent
        source: icon.stack.length ? "" : icon.source
        sourceSize: Qt.size(icon.size * 2, icon.size * 2)
        smooth: true
        mipmap: true
        visible: status === Image.Ready
    }
    Rectangle {
        visible: !picture.visible
        anchors.fill: parent
        anchors.margins: icon.size * 0.06
        radius: width * 0.225
        gradient: Gradient {
            GradientStop { position: 0; color: icon.colors[0] }
            GradientStop { position: 1; color: icon.colors[1] }
        }
        border.width: 1
        border.color: Qt.alpha("#000000", 0.08)
        // The top gloss.
        Rectangle {
            anchors.fill: parent
            radius: parent.radius
            gradient: Gradient {
                GradientStop { position: 0; color: Qt.alpha("#FFFFFF", 0.28) }
                GradientStop { position: 0.5; color: Qt.alpha("#FFFFFF", 0) }
            }
        }
        Text {
            anchors.centerIn: parent
            visible: !icon.stack.length
            text: icon.name.length ? icon.name.charAt(0).toUpperCase() : "?"
            font.family: Theme.fontFamily
            font.pixelSize: icon.size * 0.46
            font.weight: Font.Bold
            color: "#FFFFFF"
        }
        Symbol {
            anchors.centerIn: parent
            visible: icon.stack.length > 0
            name: icon.stack === "trash" ? "trash-2" : "download"
            size: icon.size * 0.5
            color: icon.stack === "trash" ? "#6B7570" : "#FFFFFF"
        }
    }
}
