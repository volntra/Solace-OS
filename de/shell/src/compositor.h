// Compositor: the shell's private line to fern-comp (fern_shell_v1,
// docs/protocols/shell.md): rounded blur behind shell surfaces and global
// shortcuts, the wallpaper, and where the active window's menus are. A QML
// singleton.
#pragma once

#include <QImage>
#include <QObject>
#include <QPointer>
#include <QQmlEngine>
#include <QRect>
#include <QtQml/qqmlregistration.h>
#include <QtWaylandClient/QWaylandClientExtension>

#include "qwayland-fern-shell-v1.h"

class QWindow;
struct wl_surface;

class ShellExtension : public QWaylandClientExtensionTemplate<ShellExtension>, public QtWayland::fern_shell_v1
{
    Q_OBJECT
public:
    ShellExtension();
signals:
    void keyBinding(int id);
    void appMenu(const QString &service, const QString &path);
    void screenshotTaken(int id, const QImage &image);
    void screenshotFailed(int id);

protected:
    void fern_shell_v1_key_binding(uint32_t id) override { emit keyBinding(int(id)); }
    void fern_shell_v1_app_menu(const QString &service, const QString &path) override { emit appMenu(service, path); }
    void fern_shell_v1_screenshot_done(uint32_t id, int32_t fd, uint32_t width, uint32_t height) override;
    void fern_shell_v1_screenshot_failed(uint32_t id) override { emit screenshotFailed(int(id)); }
};

class Compositor : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    Q_PROPERTY(bool active READ isActive NOTIFY activeChanged)
    // The active window's menus (com.canonical.dbusmenu on the session
    // bus), as the app claims; empty when it exports none.
    Q_PROPERTY(QString appMenuService READ appMenuService NOTIFY appMenuChanged)
    Q_PROPERTY(QString appMenuPath READ appMenuPath NOTIFY appMenuChanged)

public:
    explicit Compositor(QObject *parent = nullptr);
    static Compositor *instance();
    static Compositor *create(QQmlEngine *, QJSEngine *);

    bool isActive() const;
    QString appMenuService() const { return m_menuService; }
    QString appMenuPath() const { return m_menuPath; }
    void setBlur(QWindow *window, const QRect &rect, int radius);
    void unsetBlur(QWindow *window);

    // A global shortcut: modifiers are shift 1, ctrl 2, alt 4, super 8.
    Q_INVOKABLE void bindKey(int id, int modifiers, int keysym);
    // The wallpaper (the compositor crossfades to it).
    Q_INVOKABLE void setWallpaper(const QString &path);
    // Night Shift: colours as warm as this many kelvin; 0 is neutral.
    Q_INVOKABLE void setColorTemperature(int kelvin);
    // A screenshot of this rectangle of the desktop (empty: the display
    // under the pointer) on the next frame; screenshotTaken or
    // screenshotFailed follow with the returned id (-1: no compositor).
    Q_INVOKABLE int screenshot(const QRect &area);

    static wl_surface *surfaceOf(QWindow *window);

signals:
    void activeChanged();
    void keyBinding(int id);
    void appMenuChanged();
    void screenshotTaken(int id, const QImage &image);
    void screenshotFailed(int id);

private:
    ShellExtension *m_ext;
    QList<std::tuple<int, int, int>> m_bindings;
    QString m_wallpaper;
    QString m_menuService, m_menuPath;
    int m_kelvin = 0;
    int m_lastShot = 0;
    bool m_wallpaperSet = false;
};
