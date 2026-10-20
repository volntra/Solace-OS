import QtQuick
import Fern.Design
import Fern.Shell

// Search (Super+Space): type, and the answer comes first: a sum or a
// conversion, then apps, then actions (dark mode, Do Not Disturb, About,
// restart). Arrow keys pick, Return opens, Escape closes. Files join when
// the file indexer does.
Material {
    id: search
    signal done()
    signal action(string name)
    kind: "popover"
    radius: Theme.radiusPanel
    width: 640
    height: field.height + 24 + (results.count ? list.height + 12 : 0)

    readonly property string query: field.text.trim()
    readonly property string answer: Calculator.evaluate(query)
    readonly property var actions: [
        { name: "dark", title: qsTr("Dark Mode"), symbol: "moon", words: "dark mode appearance night theme" },
        { name: "light", title: qsTr("Light Mode"), symbol: "sun", words: "light mode appearance day theme" },
        { name: "dnd", title: Notifications.dnd ? qsTr("Turn Off Do Not Disturb") : qsTr("Turn On Do Not Disturb"), symbol: "bell-off", words: "do not disturb dnd focus notifications quiet" },
        { name: "screenshot", title: qsTr("Take a Screenshot"), symbol: "image", words: "screenshot screen shot capture picture" },
        { name: "nightshift", title: qsTr("Night Shift"), symbol: "sun-moon", words: "night shift warm colours colors evening" },
        { name: "about", title: qsTr("About Fern"), symbol: "info", words: "about fern system version computer" },
        { name: "restart", title: qsTr("Restart…"), symbol: "rotate-ccw", words: "restart reboot", enabled: System.canPower },
        { name: "shutdown", title: qsTr("Shut Down…"), symbol: "power", words: "shut down power off turn off", enabled: System.canPower }
    ]
    // Every row: {kind: "answer"|"app"|"action", ...}.
    readonly property var rows: {
        const out = []
        if (!query.length)
            return out
        if (answer.length)
            out.push({ kind: "answer", title: answer, subtitle: qsTr("Return copies it") })
        for (const a of Apps.search(query, 6))
            out.push({ kind: "app", id: a.id, title: a.name, subtitle: a.comment, icon: a.icon })
        const q = query.toLowerCase()
        for (const a of actions)
            if (a.enabled !== false && (a.title.toLowerCase().includes(q) || a.words.split(" ").some(w => w.startsWith(q))))
                out.push({ kind: "action", name: a.name, title: a.title, symbol: a.symbol, subtitle: qsTr("Action") })
        return out
    }
    property int current: 0
    onRowsChanged: current = 0

    function run(row) {
        if (!row)
            return
        if (row.kind === "answer") {
            clipboard.text = row.title.replace(/^= /, "")
            clipboard.selectAll()
            clipboard.copy()
        } else if (row.kind === "app") {
            Apps.launch(row.id)
        } else if (row.name === "dark" || row.name === "light") {
            Theme.appearance.theme = row.name
        } else if (row.name === "restart") {
            System.restart()
        } else if (row.name === "shutdown") {
            System.shutDown()
        }
        action(row.name ?? row.kind)
    }
    TextEdit { id: clipboard; visible: false }

    SearchField {
        id: field
        x: 12
        y: 12
        width: parent.width - 24
        height: 44
        font.pixelSize: Theme.size(20)
        placeholderText: qsTr("Search apps, actions, sums")
        focus: true
        Component.onCompleted: forceActiveFocus()
        Keys.onDownPressed: search.current = Math.min(search.rows.length - 1, search.current + 1)
        Keys.onUpPressed: search.current = Math.max(0, search.current - 1)
        Keys.onReturnPressed: search.run(search.rows[search.current])
        Keys.onEnterPressed: search.run(search.rows[search.current])
        Keys.onEscapePressed: search.done()
    }

    ListView {
        id: list
        x: 6
        y: field.y + field.height + 8
        width: parent.width - 12
        height: Math.min(contentHeight, 7 * 48)
        clip: true
        interactive: contentHeight > height
        model: search.rows
        currentIndex: search.current
        delegate: MouseArea {
            id: row
            required property var modelData
            required property int index
            width: list.width
            height: 48
            hoverEnabled: true
            onEntered: search.current = index
            onClicked: search.run(modelData)
            readonly property bool selected: index === search.current
            Rectangle {
                anchors.fill: parent
                anchors.margins: 2
                radius: Theme.radiusCard
                color: row.selected ? Theme.accent : "transparent"
            }
            AppIcon {
                id: rowIcon
                visible: row.modelData.kind === "app"
                x: 12
                anchors.verticalCenter: parent.verticalCenter
                size: 32
                appId: row.modelData.id ?? ""
                name: row.modelData.title
                source: row.modelData.icon ?? ""
            }
            Rectangle {
                visible: row.modelData.kind !== "app"
                x: 12
                anchors.verticalCenter: parent.verticalCenter
                width: 32; height: 32; radius: 8
                color: row.selected ? Qt.alpha(Theme.textOnAccent, 0.18) : Theme.fill
                Symbol {
                    anchors.centerIn: parent
                    size: 18
                    name: row.modelData.kind === "answer" ? "calculator" : row.modelData.symbol ?? "sparkles"
                    color: row.selected ? Theme.textOnAccent : Theme.text
                }
            }
            Column {
                x: 56
                anchors.verticalCenter: parent.verticalCenter
                width: parent.width - 70
                Label {
                    text: row.modelData.title
                    style: row.modelData.kind === "answer" ? "title3" : "body"
                    font.weight: Font.DemiBold
                    width: parent.width
                    color: row.selected ? Theme.textOnAccent : Theme.text
                }
                Label {
                    text: row.modelData.subtitle ?? ""
                    visible: text.length > 0
                    style: "subheadline"
                    width: parent.width
                    color: row.selected ? Qt.alpha(Theme.textOnAccent, 0.85) : Theme.secondaryText
                }
            }
        }
    }
    readonly property alias results: list
}
