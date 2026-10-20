// The design system's QML tests (tests/tst_*.qml), run with Qt Quick Test
// against the freshly built module: `just design-test`.
#include <QQmlEngine>
#include <QtQuickTest>

class Setup : public QObject
{
    Q_OBJECT
public slots:
    void qmlEngineAvailable(QQmlEngine *engine) { engine->addImportPath(QStringLiteral(FERN_QML_DIR)); }
};

QUICK_TEST_MAIN_WITH_SETUP(design, Setup)

#include "main.moc"
