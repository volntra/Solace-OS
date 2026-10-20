// fern-shell: the desktop's menu bar, dock, search, notifications and
// control center. fern-comp starts it (and restarts it if it exits) on a
// private connection that trusts it with the shell's protocols
// (docs/protocols/shell.md).
#include <QDir>
#include <QFontDatabase>
#include <QGuiApplication>
#include <QQmlApplicationEngine>

int main(int argc, char *argv[])
{
    QGuiApplication app(argc, argv);
    app.setApplicationName(QStringLiteral("fern-shell"));
    app.setOrganizationName(QStringLiteral("Fern"));
    app.setQuitOnLastWindowClosed(false);

    // Development runs bring Inter along (`just fonts`); the system has it
    // installed.
    const QString fonts = qEnvironmentVariable("FERN_FONTS");
    if (!fonts.isEmpty())
        for (const QString &f : QDir(fonts).entryList({QStringLiteral("*.ttf")}))
            QFontDatabase::addApplicationFont(QDir(fonts).filePath(f));
    QFont ui(QStringLiteral("Inter"));
    ui.setPixelSize(13);
    app.setFont(ui);

    QQmlApplicationEngine engine;
    QObject::connect(&engine, &QQmlApplicationEngine::objectCreationFailed, &app, [] { QCoreApplication::exit(1); },
                     Qt::QueuedConnection);
    engine.loadFromModule("Fern.Shell", "Main");
    return app.exec();
}
