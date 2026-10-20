import QtQuick
import Fern.Design

// A drop-down menu: `items` are {text, shortcut, enabled, checked, act,
// children, opened} or {separator: true}. An item with `children` opens a
// submenu beside it (on hover, or the Right key; Left closes it), calling
// its `opened` first. Arrow keys move, Return chooses, Escape closes;
// Left and Right past the menu's ends go to the next menu in the bar.
Material {
    id: menu
    property var items: []
    // The menu this one is a submenu of.
    property Item parentMenu: null
    signal done()
    kind: "menu"
    radius: Theme.radiusMenu
    width: Math.max(220, column.implicitWidth)
    height: column.implicitHeight + 10
    property int highlighted: -1
    // The row whose submenu is open, or -1.
    property int openSub: -1
    onItemsChanged: {
        highlighted = -1
        openSub = -1
    }

    focus: true
    Keys.onDownPressed: move(1)
    Keys.onUpPressed: move(-1)
    Keys.onReturnPressed: choose(highlighted)
    Keys.onEnterPressed: choose(highlighted)
    // Right opens a submenu; otherwise (like Left in a top menu) it goes up
    // to the menu bar, which moves to the next menu.
    Keys.onRightPressed: event => {
        if (hasChildren(highlighted))
            openSubmenu(highlighted, true)
        else
            event.accepted = false
    }
    Keys.onLeftPressed: event => {
        if (parentMenu) {
            parentMenu.openSub = -1
            parentMenu.forceActiveFocus()
        } else {
            event.accepted = false
        }
    }
    function hasChildren(i) {
        const it = items[i]
        return !!it && !it.separator && it.enabled !== false && (it.children?.length ?? 0) > 0
    }
    function move(d) {
        for (let i = 1; i <= items.length; i++) {
            const n = (highlighted + d * i + items.length * 2) % items.length
            if (!items[n].separator && items[n].enabled !== false) {
                highlighted = n
                return
            }
        }
    }
    function openSubmenu(i, focusIt) {
        if (openSub !== i) {
            const it = items[i]
            if (it.opened)
                it.opened()
            openSub = i
        }
        if (focusIt && sub.item) {
            sub.item.highlighted = -1
            sub.item.move(1)
            sub.item.forceActiveFocus()
        }
    }
    function choose(i) {
        if (hasChildren(i)) {
            openSubmenu(i, true)
            return
        }
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
            id: rows
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
                        checked: modelData.checked === true
                        submenu: menu.hasChildren(index)
                        enabled: modelData.enabled !== false
                        highlighted: (containsMouse || menu.highlighted === index || menu.openSub === index) && enabled
                        onContainsMouseChanged: if (containsMouse) {
                            menu.highlighted = index
                            if (menu.hasChildren(index))
                                menu.openSubmenu(index, false)
                            else
                                menu.openSub = -1
                        }
                        onTriggered: menu.choose(index)
                    }
                }
            }
        }
    }

    // The open submenu, beside its row (or on the left when there is no
    // room on the right).
    Loader {
        id: sub
        active: menu.openSub >= 0 && menu.hasChildren(menu.openSub)
        readonly property Item rowItem: active ? rows.itemAt(menu.openSub) : null
        // By URL: a component cannot name itself inline.
        source: "MenuPanel.qml"
        onLoaded: {
            item.parentMenu = menu
            item.items = Qt.binding(() => menu.items[menu.openSub]?.children ?? [])
            item.done.connect(menu.done)
        }
        x: {
            if (!item)
                return 0
            const right = menu.width - 4
            const room = menu.Window.width - menu.mapToItem(null, right, 0).x
            return room >= item.width + 8 ? right : 4 - item.width
        }
        y: rowItem ? rowItem.y : 0
    }
}
