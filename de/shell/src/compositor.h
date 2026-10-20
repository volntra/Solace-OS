// Compositor: the shell's private line to fern-comp (fern_shell_v1,
// docs/protocols/shell.md): rounded blur behind shell surfaces and global
// shortcuts. A QML singleton.
#pragma once

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

protected:
    void fern_shell_v1_key_binding(uint32_t id) override { emit keyBinding(int(id)); }
};

class Compositor : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    Q_PROPERTY(bool active READ isActive NOTIFY activeChanged)

public:
    explicit Compositor(QObject *parent = nullptr);
    static Compositor *instance();
    static Compositor *create(QQmlEngine *, QJSEngine *);

    bool isActive() const;
    void setBlur(QWindow *window, const QRect &rect, int radius);
    void unsetBlur(QWindow *window);

    // A global shortcut: modifiers are shift 1, ctrl 2, alt 4, super 8.
    Q_INVOKABLE void bindKey(int id, int modifiers, int keysym);
    // The wallpaper (the compositor crossfades to it).
    Q_INVOKABLE void setWallpaper(const QString &path);

    static wl_surface *surfaceOf(QWindow *window);

signals:
    void activeChanged();
    void keyBinding(int id);

private:
    ShellExtension *m_ext;
    QList<std::tuple<int, int, int>> m_bindings;
    QString m_wallpaper;
    bool m_wallpaperSet = false;
};
