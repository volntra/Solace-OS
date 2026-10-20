# The Qt side of the desktop: the design system (design/) and the shell
# (shell/). Built by `just shell-build` for development and by the
# fern-shell recipe for the image.
cmake_minimum_required(VERSION 3.22)
project(fern-desktop LANGUAGES C CXX)

find_package(Qt6 6.8 REQUIRED COMPONENTS Core Gui GuiPrivate Qml Quick QuickTemplates2 DBus Svg WaylandClient)
qt_standard_project_setup(REQUIRES 6.8)
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
# Every QML module lands in one tree, so a development build runs with
# QML_IMPORT_PATH=<build>/qml.
set(QT_QML_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/qml)
# Where Qt keeps QML modules on the system (lib/qt6/qml in our layout).
set(FERN_QML_INSTALL_DIR "lib/qt6/qml" CACHE STRING "QML module install directory, relative to the prefix")

option(FERN_BUILD_TESTS "Build the QML tests (needs Qt Quick Test)" ON)

add_subdirectory(design)
add_subdirectory(shell)
