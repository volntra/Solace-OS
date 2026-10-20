import QtQuick
import QtTest
import Fern.Design

Item {
    width: 400
    height: 300

    Toggle { id: toggle; y: 0 }
    SegmentedControl { id: segments; y: 40; model: ["Light", "Dark", "Auto"] }
    Slider { id: slider; y: 80; from: 0; to: 100; value: 50 }
    Button { id: button; y: 120; text: "Install"; kind: "primary" }
    SearchField { id: search; y: 160; width: 200 }
    Checkbox { id: checkbox; y: 200; text: "Remember" }
    MenuItem { id: menuItem; y: 240; width: 200; text: "About Fern" }

    SignalSpy { id: clicked; target: button; signalName: "clicked" }
    SignalSpy { id: activated; target: segments; signalName: "activated" }
    SignalSpy { id: triggered; target: menuItem; signalName: "triggered" }

    TestCase {
        name: "Controls"
        when: windowShown

        function test_toggle() {
            verify(!toggle.checked)
            mouseClick(toggle)
            verify(toggle.checked)
            mouseClick(toggle)
            verify(!toggle.checked)
        }

        function test_segmented_control() {
            compare(segments.currentIndex, 0)
            segments.forceActiveFocus()
            keyClick(Qt.Key_Right)
            compare(segments.currentIndex, 1)
            compare(activated.count, 1)
            keyClick(Qt.Key_Right)
            keyClick(Qt.Key_Right)
            compare(segments.currentIndex, 2)
            mouseClick(segments, 10, segments.height / 2)
            compare(segments.currentIndex, 0)
        }

        function test_slider_keys() {
            slider.forceActiveFocus()
            keyClick(Qt.Key_Right)
            compare(slider.value, 55)
            keyClick(Qt.Key_Left)
            keyClick(Qt.Key_Left)
            compare(slider.value, 45)
        }

        function test_button() {
            mouseClick(button)
            compare(clicked.count, 1)
            button.forceActiveFocus(Qt.TabFocusReason)
            verify(button.activeFocus)
        }

        function test_search_field_clears() {
            search.forceActiveFocus()
            keyClick(Qt.Key_F)
            keyClick(Qt.Key_E)
            compare(search.text, "fe")
            // The clear button sits at the right end.
            mouseClick(search, search.width - search.height / 2, search.height / 2)
            compare(search.text, "")
        }

        function test_checkbox_and_menu_item() {
            mouseClick(checkbox)
            verify(checkbox.checked)
            mouseMove(menuItem, 20, 10)
            verify(menuItem.highlighted)
            mouseClick(menuItem)
            compare(triggered.count, 1)
            menuItem.enabled = false
            mouseClick(menuItem)
            compare(triggered.count, 1)
        }

        function test_symbols_render() {
            const s = Qt.createQmlObject('import Fern.Design; Symbol { name: "search"; size: 24 }', parent)
            tryCompare(s, "status", Image.Ready)
            compare(s.sourceSize.width, 24 * Screen.devicePixelRatio)
            s.destroy()
        }
    }
}
