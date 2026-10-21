// Adaptive icons in QML (CLAUDE.md 6.9, docs/formats/icon.md), drawn by
// the fern-icons engine (de/icons) through its C API:
//
//   image://fernicon/MODE/RRGGBB/ID[/LEGACY]
//
// MODE is default, dark, tinted, wallpaper or clear; RRGGBB the accent;
// ID the icon's id (an app id, folder.downloads, ...); LEGACY an optional
// absolute path to the app's freedesktop icon, used when ID has no
// .fernicon. Icons.url() builds these. Icons are drawn off the GUI thread
// and cached on disk (~/.cache/fern/icons).
#pragma once

#include <QColor>
#include <QObject>
#include <QQuickAsyncImageProvider>
#include <QtQml/qqmlregistration.h>

struct FernIcons;

// The engine every provider in this process shares. Icons come from
// $FERN_ICONS_DIRS (colon-separated, for development) or the standard
// places ($XDG_DATA_HOME and $XDG_DATA_DIRS, then fern/icons).
FernIcons *fernIconsEngine();

class IconProvider : public QQuickAsyncImageProvider
{
public:
    QQuickImageResponse *requestImageResponse(const QString &id, const QSize &requestedSize) override;
};

// Helpers for QML: icon URLs and colour tones.
class Icons : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON

public:
    using QObject::QObject;

    // The image URL for an icon. `legacy` is a file URL or path (or empty).
    Q_INVOKABLE static QString url(const QString &id, const QString &legacy, const QString &mode, const QColor &accent);
    // `color` at another HCT tone (0 black, 100 white), hue kept.
    Q_INVOKABLE static QColor tone(const QColor &color, double tone);
};

// The accent a wallpaper suggests (HCT scoring of its colours), worked
// out off the GUI thread whenever `source` changes.
class WallpaperAccent : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(QString source READ source WRITE setSource NOTIFY sourceChanged)
    Q_PROPERTY(QColor color READ color NOTIFY colorChanged)
    Q_PROPERTY(bool ready READ ready NOTIFY colorChanged)

public:
    using QObject::QObject;

    QString source() const { return m_source; }
    void setSource(const QString &source);
    QColor color() const { return m_color; }
    bool ready() const { return m_ready; }

    // The accent of a wallpaper file (blocking); invalid if unreadable.
    static QColor compute(const QString &path);

signals:
    void sourceChanged();
    void colorChanged();

private:
    QString m_source;
    QColor m_color{0x2F, 0x7A, 0x50};
    bool m_ready = false;
    int m_generation = 0;
};
