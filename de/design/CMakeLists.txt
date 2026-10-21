# Fern.Design: the QML design system every shell component and app imports
# (CLAUDE.md 6.7, docs/design/HIG.md).

# The adaptive icon engine (de/icons, a Rust library with a C API). In
# development: `cargo build --release -p fern-icons` (just shell-build does).
set(FERN_ICONS_LIB_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../../target/release" CACHE PATH "Where libfernicons.so is")
set(FERN_ICONS_INCLUDE_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../icons/include" CACHE PATH "Where fern-icons.h is")
find_library(FERN_ICONS_LIBRARY fernicons PATHS ${FERN_ICONS_LIB_DIR} NO_DEFAULT_PATH REQUIRED)

qt_add_library(ferndesign SHARED)
# Before qt_add_qml_module, which reads it.
set_source_files_properties(qml/Theme.qml PROPERTIES QT_QML_SINGLETON_TYPE TRUE)
qt_add_qml_module(ferndesign
    URI Fern.Design
    VERSION 1.0
    PLUGIN_TARGET ferndesignplugin
    NO_PLUGIN_OPTIONAL
    NO_GENERATE_PLUGIN_SOURCE
    CLASS_NAME FernDesignPlugin
    SOURCES
        src/settingsdomain.h src/settingsdomain.cpp
        src/motion.h src/motion.cpp
        src/symbolprovider.h src/symbolprovider.cpp
        src/icons.h src/icons.cpp
    QML_FILES
        qml/Theme.qml
        qml/Label.qml
        qml/Symbol.qml
        qml/Material.qml
        qml/Button.qml
        qml/Toggle.qml
        qml/Checkbox.qml
        qml/SegmentedControl.qml
        qml/Slider.qml
        qml/TextField.qml
        qml/SearchField.qml
        qml/MenuItem.qml
        qml/MenuSeparator.qml
)
target_sources(ferndesignplugin PRIVATE src/plugin.cpp)
target_compile_definitions(ferndesign PRIVATE FERN_SOURCE_DIR="${CMAKE_CURRENT_SOURCE_DIR}")
target_link_libraries(ferndesign PRIVATE Qt6::Quick Qt6::DBus Qt6::Svg ${FERN_ICONS_LIBRARY})
target_link_libraries(ferndesignplugin PRIVATE Qt6::Quick ferndesign)
target_include_directories(ferndesign PRIVATE src ${FERN_ICONS_INCLUDE_DIR})
target_include_directories(ferndesignplugin PRIVATE ${FERN_ICONS_INCLUDE_DIR})
target_include_directories(ferndesignplugin PRIVATE src)

install(TARGETS ferndesign LIBRARY DESTINATION lib)
install(TARGETS ferndesignplugin LIBRARY DESTINATION ${FERN_QML_INSTALL_DIR}/Fern/Design)
install(FILES
    ${CMAKE_BINARY_DIR}/qml/Fern/Design/qmldir
    ${CMAKE_BINARY_DIR}/qml/Fern/Design/ferndesign.qmltypes
    DESTINATION ${FERN_QML_INSTALL_DIR}/Fern/Design)
install(DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/../../assets/symbols/lucide/
                  ${CMAKE_CURRENT_SOURCE_DIR}/../../assets/symbols/fern/
    DESTINATION share/fern/symbols)

if(FERN_BUILD_TESTS)
    find_package(Qt6 REQUIRED COMPONENTS QuickTest)
    qt_add_executable(tst_design tests/main.cpp)
    target_link_libraries(tst_design PRIVATE Qt6::QuickTest Qt6::Quick)
    target_compile_definitions(tst_design PRIVATE
        QUICK_TEST_SOURCE_DIR="${CMAKE_CURRENT_SOURCE_DIR}/tests"
        FERN_QML_DIR="${CMAKE_BINARY_DIR}/qml"
        FERN_ICONS_SET_DIR="${CMAKE_CURRENT_SOURCE_DIR}/../icons/set/icons"
        FERN_TEST_PICTURE="${CMAKE_CURRENT_SOURCE_DIR}/../../assets/brand/fern-icon-512.png")
    add_dependencies(tst_design ferndesign ferndesignplugin)
endif()
