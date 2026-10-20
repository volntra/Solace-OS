#include "compositor.h"

#include <QGuiApplication>

#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>
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

void ShellExtension::fern_shell_v1_screenshot_done(uint32_t id, int32_t fd, uint32_t width, uint32_t height)
{
    // Checked before mapping: a short file would fault on read.
    const qint64 bytes = qint64(width) * height * 4;
    struct stat st {};
    QImage image;
    if (width > 0 && height > 0 && width <= 16384 && height <= 16384 && fstat(fd, &st) == 0 && st.st_size >= bytes) {
        void *p = mmap(nullptr, size_t(bytes), PROT_READ, MAP_PRIVATE, fd, 0);
        if (p != MAP_FAILED) {
            image = QImage(static_cast<const uchar *>(p), int(width), int(height), int(width) * 4,
                           QImage::Format_RGBA8888)
                        .convertToFormat(QImage::Format_RGB32);
            munmap(p, size_t(bytes));
        }
    }
    close(fd);
    if (image.isNull())
        emit screenshotFailed(int(id));
    else
        emit screenshotTaken(int(id), image);
}

Compositor::Compositor(QObject *parent)
    : QObject(parent)
    , m_ext(new ShellExtension)
{
    s_instance = this;
    m_ext->setParent(this);
    connect(m_ext, &ShellExtension::keyBinding, this, &Compositor::keyBinding);
    connect(m_ext, &ShellExtension::screenshotTaken, this, &Compositor::screenshotTaken);
    connect(m_ext, &ShellExtension::screenshotFailed, this, &Compositor::screenshotFailed);
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
        if (m_ext->isActive() && m_kelvin > 0)
            m_ext->set_color_temperature(uint32_t(m_kelvin));
        if (m_ext->isActive() && m_dark)
            m_ext->set_dark_mode(1);
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

void Compositor::setColorTemperature(int kelvin)
{
    kelvin = std::max(kelvin, 0);
    if (kelvin == m_kelvin)
        return;
    m_kelvin = kelvin;
    if (m_ext->isActive())
        m_ext->set_color_temperature(uint32_t(kelvin));
}

void Compositor::setDarkMode(bool dark)
{
    if (dark == m_dark)
        return;
    m_dark = dark;
    if (m_ext->isActive())
        m_ext->set_dark_mode(dark ? 1 : 0);
}

int Compositor::screenshot(const QRect &area)
{
    if (!m_ext->isActive())
        return -1;
    const int id = ++m_lastShot;
    m_ext->screenshot(uint32_t(id), area.x(), area.y(), std::max(area.width(), 0), std::max(area.height(), 0));
    return id;
}

void Compositor::bindKey(int id, int modifiers, int keysym)
{
    m_bindings.append({id, modifiers, keysym});
    if (m_ext->isActive())
        m_ext->bind_key(uint32_t(id), uint32_t(modifiers), uint32_t(keysym));
}
