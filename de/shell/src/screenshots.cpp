#include "screenshots.h"

#include <QDir>
#include <QFileInfo>
#include <QStandardPaths>

QString Screenshots::folder()
{
    return QStandardPaths::writableLocation(QStandardPaths::PicturesLocation) + QStringLiteral("/Screenshots");
}

QString Screenshots::fileName(const QDateTime &when)
{
    return QStringLiteral("Screenshot %1 at %2.png")
        .arg(when.toString(QStringLiteral("yyyy-MM-dd")), when.toString(QStringLiteral("HH.mm.ss")));
}

QString Screenshots::freePath(const QString &dir, const QString &name)
{
    QString path = QDir(dir).filePath(name);
    const QString base = QFileInfo(name).completeBaseName();
    for (int n = 2; QFileInfo::exists(path); ++n)
        path = QDir(dir).filePath(QStringLiteral("%1 %2.png").arg(base).arg(n));
    return path;
}

QString Screenshots::save(const QImage &image, const QString &dir, const QDateTime &when)
{
    if (image.isNull() || !QDir().mkpath(dir))
        return {};
    const QString path = freePath(dir, fileName(when));
    return image.save(path, "PNG") ? path : QString();
}

QString Screenshots::saveNow(const QImage &image) const
{
    return save(image, folder(), QDateTime::currentDateTime());
}
