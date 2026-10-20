#include "compositor.h"

#include <QGuiApplication>
#include <QWindow>
#include <qpa/qplatformnativeinterface.h>

namespace {
Compositor *s_instance = nullptr;
}

ShellExtension::ShellExtension()
    : QWaylandClientExtensionTemplate<ShellExtension>(1)
{
    initialize();
}

Compositor::Compositor(QObject *parent)
    : QObject(parent)
    , m_ext(new ShellExtension)
{
    s_instance = this;
    m_ext->setParent(this);
    connect(m_ext, &ShellExtension::keyBinding, this, &Compositor::keyBinding);
    connect(m_ext, &ShellExtension::appMenu, this, [this](const QString &service, const QString &path) {
        if (service == m_menuService && path == m_menuPath)
            return;
        m_menuService = service;
        m_menuPath = path;
        emit appMenuChanged();
    });
    connect(m_ext, &QWaylandClientExtension::activeChanged, this, [this] {
        // Re-send our shortcuts if the global (re)appeared.
        if (m_ext->isActive())
            for (const auto &[id, mods, sym] : std::as_const(m_bindings))
                m_ext->bind_key(uint32_t(id), uint32_t(mods), uint32_t(sym));
        if (m_ext->isActive() && m_wallpaperSet)
            m_ext->set_wallpaper(m_wallpaper);
        emit activeChanged();
    });
}

Compositor *Compositor::instance()
{
    return s_instance;
}

Compositor *Compositor::create(QQmlEngine *, QJSEngine *)
{
    if (!s_instance)
        new Compositor;
    QJSEngine::setObjectOwnership(s_instance, QJSEngine::CppOwnership);
    return s_instance;
}

bool Compositor::isActive() const
{
    return m_ext->isActive();
}

wl_surface *Compositor::surfaceOf(QWindow *window)
{
    QPlatformNativeInterface *native = QGuiApplication::platformNativeInterface();
    if (!native || !window)
        return nullptr;
    return static_cast<wl_surface *>(native->nativeResourceForWindow("surface", window));
}

void Compositor::setBlur(QWindow *window, const QRect &rect, int radius)
{
    wl_surface *s = surfaceOf(window);
    if (!s || !m_ext->isActive())
        return;
    m_ext->set_blur(s, rect.x(), rect.y(), rect.width(), rect.height(), radius);
}

void Compositor::unsetBlur(QWindow *window)
{
    wl_surface *s = surfaceOf(window);
    if (!s || !m_ext->isActive())
        return;
    m_ext->unset_blur(s);
}

void Compositor::setWallpaper(const QString &path)
{
    m_wallpaper = path;
    m_wallpaperSet = true;
    if (m_ext->isActive())
        m_ext->set_wallpaper(path);
}

void Compositor::bindKey(int id, int modifiers, int keysym)
{
    m_bindings.append({id, modifiers, keysym});
    if (m_ext->isActive())
        m_ext->bind_key(uint32_t(id), uint32_t(modifiers), uint32_t(keysym));
}
