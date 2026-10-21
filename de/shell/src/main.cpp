// fern-shell: the desktop's menu bar, dock, search, notifications and
// control center. fern-comp starts it (and restarts it if it exits) on a
// private connection that trusts it with the shell's protocols
// (docs/protocols/shell.md).
#include <QDir>
#include <QFontDatabase>
#include <QGuiApplication>
#include <QQmlApplicationEngine>
#include <QSurfaceFormat>

int main(int argc, char *argv[])
{
    // One GL context's shaders and textures for all the shell's windows: a
    // panel opening for the first time does not compile everything again.
    QCoreApplication::setAttribute(Qt::AA_ShareOpenGLContexts);
    // No emulated blocking swap: with a swap interval above 0, QtWayland's
    // GL swap waits 100 ms for the last frame callback and, when the
    // compositor is slower than that (software rendering, a long frame),
    // calls the window hidden until a callback comes; a window that is
    // idle by then stays hidden. Frame callbacks still pace drawing.
    QSurfaceFormat format = QSurfaceFormat::defaultFormat();
    format.setSwapInterval(0);
    QSurfaceFormat::setDefaultFormat(format);
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
