# fern-shell: the menu bar, dock, search, notifications and control center
# (CLAUDE.md 6.8). One process, started by fern-comp with --shell.
find_package(LayerShellQt REQUIRED)

qt_add_executable(fern-shell src/main.cpp)
qt_add_qml_module(fern-shell
    URI Fern.Shell
    VERSION 1.0
    SOURCES
        src/shellwindow.h src/shellwindow.cpp
        src/compositor.h src/compositor.cpp
        src/windows.h src/windows.cpp
        src/apps.h src/apps.cpp
        src/notifications.h src/notifications.cpp
        src/calculator.h src/calculator.cpp
        src/system.h src/system.cpp
        src/media.h src/media.cpp
        src/devices.h src/devices.cpp
    QML_FILES
        qml/Main.qml
        qml/MenuBar.qml
        qml/Dock.qml
        qml/DockItem.qml
        qml/AppIcon.qml
        qml/Popups.qml
        qml/MenuPanel.qml
        qml/ControlCenter.qml
        qml/ControlTile.qml
        qml/Search.qml
        qml/Banners.qml
        qml/NotificationCard.qml
        qml/NotificationCenter.qml
        qml/AboutPanel.qml
        qml/StackPanel.qml
        qml/Hud.qml
)
qt6_generate_wayland_protocol_client_sources(fern-shell
    FILES
        ${CMAKE_CURRENT_SOURCE_DIR}/../protocols/fern-shell-v1.xml
        ${CMAKE_CURRENT_SOURCE_DIR}/protocols/wlr-foreign-toplevel-management-unstable-v1.xml
)
target_include_directories(fern-shell PRIVATE src)
target_link_libraries(fern-shell PRIVATE
    Qt6::Quick Qt6::DBus Qt6::WaylandClient Qt6::GuiPrivate LayerShellQtInterface)
install(TARGETS fern-shell RUNTIME DESTINATION bin)

if(FERN_BUILD_TESTS)
    find_package(Qt6 REQUIRED COMPONENTS Test)
    qt_add_executable(tst_calculator tests/tst_calculator.cpp src/calculator.cpp)
    target_include_directories(tst_calculator PRIVATE src)
    target_link_libraries(tst_calculator PRIVATE Qt6::Test Qt6::Qml)
    qt_add_executable(tst_apps tests/tst_apps.cpp src/apps.cpp)
    target_include_directories(tst_apps PRIVATE src)
    target_link_libraries(tst_apps PRIVATE Qt6::Test Qt6::Qml Qt6::Gui)
endif()
