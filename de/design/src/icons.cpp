#include "icons.h"

#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QImage>
#include <QMutex>
#include <QPointer>
#include <QRunnable>
#include <QThreadPool>
#include <QUrl>

#include <fern-icons.h>

FernIcons *fernIconsEngine()
{
    // The standard places, or $FERN_ICONS_DIRS (the engine reads it).
    static FernIcons *engine = fern_icons_new(nullptr, 0, "");
    return engine;
}

namespace {

const QStringList modes = {QStringLiteral("default"), QStringLiteral("dark"), QStringLiteral("tinted"),
                           QStringLiteral("wallpaper"), QStringLiteral("clear")};

// The icon for apps that have none at all.
const char *const fallbackId = "fern.app";

class IconResponse : public QQuickImageResponse, public QRunnable
{
public:
    IconResponse(const QString &id, const QSize &requestedSize)
        : m_id(id)
        , m_size(requestedSize)
    {
        setAutoDelete(false);
    }

    QQuickTextureFactory *textureFactory() const override
    {
        return QQuickTextureFactory::textureFactoryForImage(m_image);
    }

    QString errorString() const override { return m_error; }

    void run() override
    {
        m_image = render();
        emit finished();
    }

private:
    QImage render()
    {
        // MODE/RRGGBB/ID[/LEGACY]
        const QStringList parts = m_id.split(QLatin1Char('/'));
        if (parts.size() < 3) {
            m_error = QStringLiteral("bad icon URL: ") + m_id;
            return {};
        }
        const int mode = modes.indexOf(parts[0]);
        bool ok = false;
        const uint accent = parts[1].toUInt(&ok, 16);
        const QByteArray id = parts[2].toUtf8();
        // The legacy path is absolute: its first '/' separates it from the id.
        const QByteArray legacy = parts.size() > 3
            ? QUrl::fromPercentEncoding(QByteArray("/") + parts.mid(3).join(QLatin1Char('/')).toUtf8()).toUtf8()
            : QByteArray();
        if (mode < 0 || !ok) {
            m_error = QStringLiteral("bad icon URL: ") + m_id;
            return {};
        }
        // The requested size, within what the engine draws (128 if none).
        const int want = m_size.isValid() && m_size.width() > 0 ? std::max(m_size.width(), m_size.height()) : 128;
        const int size = std::clamp(want, 8, 1024);
        QImage image(size, size, QImage::Format_RGBA8888_Premultiplied);
        auto draw = [&](const char *which, const char *file) {
            return fern_icons_render(fernIconsEngine(), which, file, uint32_t(mode), accent, uint32_t(size), image.bits(),
                                     size_t(image.sizeInBytes()));
        };
        int result = draw(id.constData(), legacy.isEmpty() ? nullptr : legacy.constData());
        if (result == FERN_ICONS_NOT_FOUND || result == FERN_ICONS_BAD_ICON)
            result = draw(fallbackId, nullptr);
        if (result != FERN_ICONS_OK) {
            m_error = QStringLiteral("no icon for ") + QString::fromUtf8(id);
            return {};
        }
        return image;
    }

    QString m_id;
    QSize m_size;
    QImage m_image;
    QString m_error;
};

} // namespace

QQuickImageResponse *IconProvider::requestImageResponse(const QString &id, const QSize &requestedSize)
{
    auto *response = new IconResponse(id, requestedSize);
    // A pool of its own, so slow icons never hold up other Qt work.
    static QThreadPool *pool = [] {
        auto *p = new QThreadPool;
        p->setMaxThreadCount(std::max(2, QThread::idealThreadCount() / 2));
        return p;
    }();
    pool->start(response);
    return response;
}

QString Icons::url(const QString &id, const QString &legacy, const QString &mode, const QColor &accent)
{
    const QString m = modes.contains(mode) ? mode : QStringLiteral("default");
    const QString rgb = QStringLiteral("%1").arg(accent.isValid() ? accent.rgb() & 0xFFFFFF : 0x2F7A50, 6, 16, QLatin1Char('0'));
    QString u = QStringLiteral("image://fernicon/") + m + QLatin1Char('/') + rgb + QLatin1Char('/') + (id.isEmpty() ? QString::fromLatin1(fallbackId) : id);
    QString path = legacy;
    if (path.startsWith(QLatin1String("file:")))
        path = QUrl(path).toLocalFile();
    if (!path.isEmpty() && QDir::isAbsolutePath(path))
        u += QString::fromLatin1(QUrl::toPercentEncoding(path, "/"));
    return u;
}

QColor Icons::tone(const QColor &color, double tone)
{
    const uint rgb = fern_icons_tone(color.rgb() & 0xFFFFFF, tone);
    return QColor::fromRgb(rgb);
}

QColor WallpaperAccent::compute(const QString &path)
{
    uint32_t rgb = 0;
    const QByteArray p = QFile::encodeName(path);
    if (fern_icons_wallpaper_accent(p.constData(), &rgb) != FERN_ICONS_OK)
        return {};
    return QColor::fromRgb(rgb);
}

void WallpaperAccent::setSource(const QString &source)
{
    if (source == m_source)
        return;
    m_source = source;
    emit sourceChanged();
    const int generation = ++m_generation;
    QString path = source;
    if (path.startsWith(QLatin1String("file:")))
        path = QUrl(path).toLocalFile();
    QPointer<WallpaperAccent> self(this);
    QThreadPool::globalInstance()->start([self, path, generation] {
        const QColor c = compute(path);
        QMetaObject::invokeMethod(
            QCoreApplication::instance(),
            [self, c, generation] {
                // A newer wallpaper's answer wins; an unreadable one keeps
                // the colour before it.
                if (!self || generation != self->m_generation || !c.isValid())
                    return;
                self->m_color = c;
                self->m_ready = true;
                emit self->colorChanged();
            },
            Qt::QueuedConnection);
    });
}
