# Fern.Design: the QML design system every shell component and app imports
# (CLAUDE.md 6.7, docs/design/HIG.md).
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
target_link_libraries(ferndesign PRIVATE Qt6::Quick Qt6::DBus Qt6::Svg)
target_link_libraries(ferndesignplugin PRIVATE Qt6::Quick)
target_include_directories(ferndesign PRIVATE src)
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
        FERN_QML_DIR="${CMAKE_BINARY_DIR}/qml")
    add_dependencies(tst_design ferndesign ferndesignplugin)
endif()
