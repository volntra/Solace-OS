import QtQuick
import QtQuick.Templates as T
import Fern.Design

// Push button. `kind`: "primary" (the accent: one per view, the default
// action), "secondary" (default) or "plain" (no fill until hovered).
T.Button {
    id: control
    property string kind: "secondary"
    property string symbol: ""
    implicitWidth: Math.max(64, implicitContentWidth + leftPadding + rightPadding)
    implicitHeight: Theme.controlHeight
    leftPadding: Theme.space(3)
    rightPadding: Theme.space(3)
    font.family: Theme.fontFamily
    font.pixelSize: Theme.body
    font.weight: kind === "primary" ? Font.DemiBold : Font.Medium
    focusPolicy: Qt.StrongFocus

    readonly property color fg: !enabled ? Theme.disabledText
        : kind === "primary" ? Theme.textOnAccent : Theme.text

    contentItem: Row {
        spacing: Theme.space(1.5)
        Symbol {
            visible: control.symbol.length > 0
            name: control.symbol
            color: control.fg
            size: 14
            anchors.verticalCenter: parent.verticalCenter
        }
        Text {
            text: control.text
            font: control.font
            color: control.fg
            anchors.verticalCenter: parent.verticalCenter
        }
    }
    background: Rectangle {
        radius: Theme.radiusControl
        color: control.kind === "primary"
            ? (control.enabled ? Qt.darker(Theme.accent, control.down ? 1.18 : control.hovered ? 1.08 : 1.0) : Theme.fill)
            : control.kind === "plain" && !control.hovered && !control.down ? "transparent"
            : control.down ? Theme.fillPressed : control.hovered ? Theme.fillHover : Theme.fill
        border.width: control.visualFocus ? 3 : 0
        border.color: Theme.focusRing
        Behavior on color { ColorAnimation { duration: Theme.snappy; easing: Motion.curve } }
    }
}
