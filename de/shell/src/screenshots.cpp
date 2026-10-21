#include "screenshots.h"

#include "log.h"

#include <QCoreApplication>
#include <QDir>
#include <QFileInfo>
#include <QPointer>
#include <QStandardPaths>
#include <QThreadPool>

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

void Screenshots::saveNow(const QImage &image)
{
    const QDateTime when = QDateTime::currentDateTime();
    const QString dir = folder();
    QPointer<Screenshots> self(this);
    QThreadPool::globalInstance()->start([self, image, dir, when] {
        const QString path = save(image, dir, when);
        qCInfo(lcShell) << "screenshot saved" << path;
        // Back on the UI thread.
        QMetaObject::invokeMethod(qApp, [self, path] {
            if (!self)
                return;
            if (path.isEmpty())
                emit self->failed();
            else
                emit self->saved(path);
        });
    });
}
