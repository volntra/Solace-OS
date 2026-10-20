#include "symbolprovider.h"

#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QPainter>
#include <QRegularExpression>
#include <QSvgRenderer>
#include <QUrlQuery>

SymbolProvider::SymbolProvider()
    : QQuickImageProvider(QQuickImageProvider::Image)
{
}

QStringList SymbolProvider::directories()
{
    QStringList out;
    const QString env = qEnvironmentVariable("FERN_SYMBOLS");
    if (!env.isEmpty())
        out.append(env);
    for (const QString &d : {QStringLiteral("/usr/share/fern/symbols"),
                             QStringLiteral(FERN_SOURCE_DIR "/../../assets/symbols/lucide"),
                             QStringLiteral(FERN_SOURCE_DIR "/../../assets/symbols/fern")}) {
        if (QDir(d).exists())
            out.append(d);
    }
    return out;
}

QImage SymbolProvider::requestImage(const QString &id, QSize *size, const QSize &requestedSize)
{
    const int q = id.indexOf(QLatin1Char('?'));
    const QString name = q < 0 ? id : id.left(q);
    const QUrlQuery query(q < 0 ? QString() : id.mid(q + 1));
    QString color = query.queryItemValue(QStringLiteral("color"), QUrl::FullyDecoded);
    if (color.isEmpty())
        color = QStringLiteral("#101512");
    // Only plain names: never a path out of the symbol directory.
    static const QRegularExpression valid(QStringLiteral("^[a-z0-9-]+$"));
    QImage img(requestedSize.isValid() ? requestedSize : QSize(24, 24), QImage::Format_ARGB32_Premultiplied);
    img.fill(Qt::transparent);
    if (size)
        *size = img.size();
    if (!valid.match(name).hasMatch())
        return img;
    QFile f;
    for (const QString &d : directories()) {
        f.setFileName(d + QLatin1Char('/') + name + QStringLiteral(".svg"));
        if (f.open(QIODevice::ReadOnly))
            break;
    }
    if (!f.isOpen())
        return img;
    QByteArray svg = f.readAll();
    svg.replace("currentColor", color.toLatin1());
    QSvgRenderer renderer(svg);
    QPainter p(&img);
    p.setRenderHint(QPainter::Antialiasing);
    renderer.render(&p);
    return img;
}
