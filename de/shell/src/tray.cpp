#include "tray.h"

#include "apps.h"

#include <QBuffer>
#include <QCoreApplication>
#include <QDBusConnection>
#include <QDBusConnectionInterface>
#include <QDBusMessage>
#include <QDBusObjectPath>
#include <QDBusVariant>
#include <QDir>
#include <QFile>
#include <QImage>
#include <QUrl>
#include <QtEndian>

namespace {
const QString ItemIface = QStringLiteral("org.kde.StatusNotifierItem");
const QString WatcherName = QStringLiteral("org.kde.StatusNotifierWatcher");

QVariant unwrap(QVariant v)
{
    if (v.canConvert<QDBusVariant>())
        v = v.value<QDBusVariant>().variant();
    return v;
}
}

TrayWatcherAdaptor::TrayWatcherAdaptor(Tray *parent)
    : QDBusAbstractAdaptor(parent)
    , m_tray(parent)
{
    connect(parent, &Tray::itemRegistered, this, &TrayWatcherAdaptor::StatusNotifierItemRegistered);
    connect(parent, &Tray::itemUnregistered, this, &TrayWatcherAdaptor::StatusNotifierItemUnregistered);
}

QStringList TrayWatcherAdaptor::items() const
{
    return m_tray->keys();
}

void TrayWatcherAdaptor::RegisterStatusNotifierItem(const QString &service)
{
    m_tray->registerItem(service, m_tray->caller());
}

void TrayWatcherAdaptor::RegisterStatusNotifierHost(const QString &)
{
    // We are the only host; nothing to remember.
}

Tray::Tray(QObject *parent)
    : QAbstractListModel(parent)
{
    new TrayWatcherAdaptor(this);
    QDBusConnection bus = QDBusConnection::sessionBus();
    if (!bus.isConnected())
        return;
    bus.registerObject(QStringLiteral("/StatusNotifierWatcher"), this);
    bus.interface()->registerService(WatcherName, QDBusConnectionInterface::ReplaceExistingService,
                                     QDBusConnectionInterface::DontAllowReplacement);
    bus.interface()->registerService(QStringLiteral("org.kde.StatusNotifierHost-%1").arg(QCoreApplication::applicationPid()));
    bus.connect(QStringLiteral("org.freedesktop.DBus"), QStringLiteral("/org/freedesktop/DBus"),
                QStringLiteral("org.freedesktop.DBus"), QStringLiteral("NameOwnerChanged"), this,
                SLOT(ownerChanged(QString, QString, QString)));
}

std::pair<QString, QString> Tray::address(const QString &arg, const QString &sender)
{
    if (arg.startsWith(QLatin1Char('/')))
        return {sender, arg};
    const int slash = arg.indexOf(QLatin1Char('/'));
    if (slash > 0)
        return {arg.left(slash), arg.mid(slash)};
    return {arg, QStringLiteral("/StatusNotifierItem")};
}

QStringList Tray::keys() const
{
    QStringList out;
    for (const Item &i : m_items)
        out.append(i.key);
    return out;
}

int Tray::row(const QString &key) const
{
    for (int i = 0; i < m_items.size(); ++i)
        if (m_items.at(i).key == key)
            return i;
    return -1;
}

void Tray::registerItem(const QString &arg, const QString &sender)
{
    const auto [service, path] = address(arg, sender);
    const QString key = service + path;
    if (row(key) >= 0)
        return;
    Item item{key, service, path, {}, {}, {}, {}, false};
    read(item);
    beginInsertRows({}, int(m_items.size()), int(m_items.size()));
    m_items.append(item);
    endInsertRows();
    emit countChanged();
    emit itemRegistered(key);
    QDBusConnection bus = QDBusConnection::sessionBus();
    for (const char *sig : {"NewIcon", "NewTitle", "NewStatus", "NewAttentionIcon", "NewMenu"})
        bus.connect(service, path, ItemIface, QLatin1String(sig), this, SLOT(itemChanged()));
}

void Tray::itemChanged()
{
    const QString key = message().service() + message().path();
    for (int i = 0; i < m_items.size(); ++i) {
        Item &it = m_items[i];
        // The signal comes from the unique name; items may be known by a
        // well-known one.
        const QString owner = QDBusConnection::sessionBus().interface()->serviceOwner(it.service);
        if (it.key == key || owner + it.path == key) {
            read(it);
            emit dataChanged(index(i), index(i));
        }
    }
}

void Tray::ownerChanged(const QString &name, const QString &, const QString &newOwner)
{
    if (!newOwner.isEmpty())
        return;
    for (int i = int(m_items.size()) - 1; i >= 0; --i) {
        if (m_items.at(i).service != name)
            continue;
        const QString key = m_items.at(i).key;
        beginRemoveRows({}, i, i);
        m_items.removeAt(i);
        endRemoveRows();
        emit countChanged();
        emit itemUnregistered(key);
    }
}

QString Tray::pixmapUrl(const QDBusArgument &arg)
{
    int bestW = 0, bestH = 0;
    QByteArray best;
    arg.beginArray();
    while (!arg.atEnd()) {
        int w = 0, h = 0;
        QByteArray bytes;
        arg.beginStructure();
        arg >> w >> h >> bytes;
        arg.endStructure();
        // From the app: sizes are checked before they are multiplied.
        if (w <= 0 || h <= 0 || w > 1024 || h > 1024 || bytes.size() < qsizetype(w) * h * 4)
            continue;
        const bool better = bestW == 0 || (w <= 64 && (bestW > 64 || w > bestW)) || (bestW > 64 && w < bestW);
        if (better) {
            bestW = w;
            bestH = h;
            best = bytes;
        }
    }
    arg.endArray();
    if (bestW == 0)
        return {};
    QImage img(bestW, bestH, QImage::Format_ARGB32);
    const auto *src = reinterpret_cast<const uchar *>(best.constData());
    for (int y = 0; y < bestH; ++y) {
        auto *line = reinterpret_cast<QRgb *>(img.scanLine(y));
        for (int x = 0; x < bestW; ++x)
            line[x] = qFromBigEndian<quint32>(src + 4 * (y * bestW + x));
    }
    QByteArray png;
    QBuffer buf(&png);
    buf.open(QIODevice::WriteOnly);
    img.save(&buf, "PNG");
    return QStringLiteral("data:image/png;base64,") + QString::fromLatin1(png.toBase64());
}

void Tray::read(Item &item)
{
    QDBusMessage call = QDBusMessage::createMethodCall(item.service, item.path,
                                                       QStringLiteral("org.freedesktop.DBus.Properties"),
                                                       QStringLiteral("GetAll"));
    call << ItemIface;
    const QDBusMessage r = QDBusConnection::sessionBus().call(call, QDBus::Block, 500);
    if (r.type() != QDBusMessage::ReplyMessage || r.arguments().isEmpty())
        return;
    QVariantMap props;
    r.arguments().first().value<QDBusArgument>() >> props;
    item.title = unwrap(props.value(QStringLiteral("Title"))).toString();
    if (item.title.isEmpty())
        item.title = unwrap(props.value(QStringLiteral("Id"))).toString();
    item.status = unwrap(props.value(QStringLiteral("Status"))).toString();
    item.menu = unwrap(props.value(QStringLiteral("Menu"))).value<QDBusObjectPath>().path();
    item.itemIsMenu = unwrap(props.value(QStringLiteral("ItemIsMenu"))).toBool();
    const bool attention = item.status == QLatin1String("NeedsAttention");
    QString name = unwrap(props.value(QString::fromLatin1(attention ? "AttentionIconName" : "IconName"))).toString();
    if (name.isEmpty())
        name = unwrap(props.value(QStringLiteral("IconName"))).toString();
    item.icon.clear();
    if (!name.isEmpty()) {
        const QString themePath = unwrap(props.value(QStringLiteral("IconThemePath"))).toString();
        if (!themePath.isEmpty()) {
            for (const char *ext : {".svg", ".png"}) {
                const QString p = QDir(themePath).filePath(name + QLatin1String(ext));
                if (QFile::exists(p))
                    item.icon = QUrl::fromLocalFile(p).toString();
            }
        }
        if (item.icon.isEmpty()) {
            QStringList dirs{qEnvironmentVariable("XDG_DATA_HOME", QDir::homePath() + QStringLiteral("/.local/share"))};
            dirs += qEnvironmentVariable("XDG_DATA_DIRS", QStringLiteral("/usr/local/share:/usr/share")).split(QLatin1Char(':'));
            item.icon = Apps::findIcon(name, dirs);
        }
    }
    if (item.icon.isEmpty()) {
        const QVariant pix = unwrap(props.value(QString::fromLatin1(attention ? "AttentionIconPixmap" : "IconPixmap")));
        if (pix.canConvert<QDBusArgument>())
            item.icon = pixmapUrl(pix.value<QDBusArgument>());
    }
}

int Tray::rowCount(const QModelIndex &parent) const
{
    return parent.isValid() ? 0 : int(m_items.size());
}

QVariant Tray::data(const QModelIndex &index, int role) const
{
    if (!index.isValid() || index.row() >= m_items.size())
        return {};
    const Item &i = m_items.at(index.row());
    switch (role) {
    case KeyRole: return i.key;
    case Qt::DisplayRole:
    case TitleRole: return i.title;
    case IconRole: return i.icon;
    case StatusRole: return i.status;
    case ServiceRole: return i.service;
    case MenuRole: return i.menu;
    case ItemIsMenuRole: return i.itemIsMenu;
    default: return {};
    }
}

QHash<int, QByteArray> Tray::roleNames() const
{
    return {{KeyRole, "key"}, {TitleRole, "title"}, {IconRole, "icon"}, {StatusRole, "status"},
            {ServiceRole, "service"}, {MenuRole, "menu"}, {ItemIsMenuRole, "itemIsMenu"}};
}

void Tray::activate(const QString &key, int x, int y)
{
    const int r = row(key);
    if (r < 0)
        return;
    const Item &i = m_items.at(r);
    QDBusMessage call = QDBusMessage::createMethodCall(i.service, i.path, ItemIface, QStringLiteral("Activate"));
    call << x << y;
    QDBusConnection::sessionBus().asyncCall(call);
}
