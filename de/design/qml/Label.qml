import QtQuick
import Fern.Design

// Text in the type scale: `style` is body, headline, title1..3, callout,
// subheadline, footnote or largeTitle; `secondary` dims it.
Text {
    property string style: "body"
    property bool secondary: false
    readonly property var scale: ({
        largeTitle: [Theme.largeTitle, Font.Bold],
        title1: [Theme.title1, Font.Bold],
        title2: [Theme.title2, Font.DemiBold],
        title3: [Theme.title3, Font.DemiBold],
        headline: [Theme.headline, Font.DemiBold],
        body: [Theme.body, Font.Normal],
        callout: [Theme.callout, Font.Normal],
        subheadline: [Theme.subheadline, Font.Normal],
        footnote: [Theme.footnote, Font.Normal]
    })[style] ?? [Theme.body, Font.Normal]
    font.family: Theme.fontFamily
    font.pixelSize: scale[0]
    font.weight: scale[1]
    color: enabled ? (secondary ? Theme.secondaryText : Theme.text) : Theme.disabledText
    elide: Text.ElideRight
    verticalAlignment: Text.AlignVCenter
}
