// The design system's QML tests (tests/tst_*.qml), run with Qt Quick Test
// against the freshly built module: `just design-test`.
#include <QQmlContext>
#include <QQmlEngine>
#include <QtQuickTest>

class Setup : public QObject
{
    Q_OBJECT
public slots:
    void qmlEngineAvailable(QQmlEngine *engine)
    {
        engine->addImportPath(QStringLiteral(FERN_QML_DIR));
        // The repository's icon set, never the user's cache.
        qputenv("FERN_ICONS_DIRS", FERN_ICONS_SET_DIR);
        engine->rootContext()->setContextProperty(QStringLiteral("FernTest"), this);
    }

public:
    // A picture that is mostly green, for the wallpaper accent.
    Q_PROPERTY(QString greenPicture READ greenPicture CONSTANT)
    QString greenPicture() const { return QStringLiteral(FERN_TEST_PICTURE); }
};

QUICK_TEST_MAIN_WITH_SETUP(design, Setup)

#include "main.moc"
