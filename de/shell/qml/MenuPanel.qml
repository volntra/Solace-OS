import QtQuick
import Fern.Design

// A drop-down menu: `items` are {text, shortcut, enabled, act} or
// {separator: true}. Arrow keys move, Return chooses, Escape closes.
Material {
    id: menu
    property var items: []
    signal done()
    kind: "menu"
    radius: Theme.radiusMenu
    width: Math.max(220, column.implicitWidth)
    height: column.implicitHeight + 10
    property int highlighted: -1

    focus: true
    Keys.onDownPressed: move(1)
    Keys.onUpPressed: move(-1)
    Keys.onReturnPressed: choose(highlighted)
    Keys.onEnterPressed: choose(highlighted)
    function move(d) {
        for (let i = 1; i <= items.length; i++) {
            const n = (highlighted + d * i + items.length * 2) % items.length
            if (!items[n].separator && items[n].enabled !== false) {
                highlighted = n
                return
            }
        }
    }
    function choose(i) {
        const it = items[i]
        if (!it || it.separator || it.enabled === false)
            return
        done()
        if (it.act)
            it.act()
    }

    Column {
        id: column
        y: 5
        width: parent.width
        Repeater {
            model: menu.items
            delegate: Loader {
                required property var modelData
                required property int index
                width: column.width
                sourceComponent: modelData.separator ? sep : row
                Component { id: sep; MenuSeparator {} }
                Component {
                    id: row
                    MenuItem {
                        text: modelData.text
                        shortcut: modelData.shortcut ?? ""
                        enabled: modelData.enabled !== false
                        highlighted: (containsMouse || menu.highlighted === index) && enabled
                        onContainsMouseChanged: if (containsMouse) menu.highlighted = index
                        onTriggered: menu.choose(index)
                    }
                }
            }
        }
    }
}
