#include "notifications.h"

#include "log.h"

#include <QFileInfo>
#include <QUrl>

#include <QDBusConnection>
#include <QDBusConnectionInterface>

NotificationsAdaptor::NotificationsAdaptor(Notifications *parent)
    : QDBusAbstractAdaptor(parent)
    , m_n(parent)
{
    connect(parent, &Notifications::closedSignal, this, &NotificationsAdaptor::NotificationClosed);
    connect(parent, &Notifications::actionSignal, this, &NotificationsAdaptor::ActionInvoked);
}

QStringList NotificationsAdaptor::GetCapabilities()
{
    return {QStringLiteral("body"), QStringLiteral("actions"), QStringLiteral("persistence"),
            QStringLiteral("body-markup")};
}

uint NotificationsAdaptor::Notify(const QString &app_name, uint replaces_id, const QString &app_icon,
                                  const QString &summary, const QString &body, const QStringList &actions,
                                  const QVariantMap &hints, int expire_timeout)
{
    Notifications::Item item;
    item.app = app_name;
    item.icon = app_icon;
    item.summary = summary;
    item.body = body;
    item.actions = actions;
    item.timeout = expire_timeout;
    const QVariant urgency = hints.value(QStringLiteral("urgency"));
    item.urgent = urgency.isValid() && urgency.toInt() == 2;
    item.resident = hints.value(QStringLiteral("resident")).toBool();
    // A picture to show with it (a screenshot, a sender's photo): a local
    // file only, never fetched from anywhere.
    QString image = hints.value(QStringLiteral("image-path")).toString();
    if (image.isEmpty())
        image = hints.value(QStringLiteral("image_path")).toString();
    item.image = Notifications::localImage(image);
    if (item.icon.isEmpty())
        item.icon = image;
    return m_n->add(item, replaces_id);
}

void NotificationsAdaptor::CloseNotification(uint id)
{
    m_n->close(id, Notifications::Closed);
}

QString NotificationsAdaptor::GetServerInformation(QString &vendor, QString &version, QString &spec_version)
{
    vendor = QStringLiteral("Fern");
    version = QStringLiteral("0.1");
    spec_version = QStringLiteral("1.2");
    return QStringLiteral("Fern");
}

Notifications::Notifications(QObject *parent)
    : QAbstractListModel(parent)
{
    new NotificationsAdaptor(this);
    QDBusConnection bus = QDBusConnection::sessionBus();
    if (bus.isConnected()) {
        bus.registerObject(QStringLiteral("/org/freedesktop/Notifications"), this);
        // Our session's server: take the name over if another one has it.
        const auto reply = bus.interface()->registerService(QStringLiteral("org.freedesktop.Notifications"),
                                                            QDBusConnectionInterface::ReplaceExistingService,
                                                            QDBusConnectionInterface::DontAllowReplacement);
        m_serving = reply.isValid() && reply.value() == QDBusConnectionInterface::ServiceRegistered;
        if (!m_serving)
            qWarning("notifications: another server owns org.freedesktop.Notifications");
    }
    m_tick.setInterval(250);
    connect(&m_tick, &QTimer::timeout, this, &Notifications::expireBanners);
}

int Notifications::rowCount(const QModelIndex &parent) const
{
    return parent.isValid() ? 0 : int(m_items.size());
}

QVariant Notifications::data(const QModelIndex &index, int role) const
{
    if (!index.isValid() || index.row() >= m_items.size())
        return {};
    const Item &n = m_items.at(index.row());
    switch (role) {
    case IdRole: return n.id;
    case AppRole: return n.app;
    case IconRole: return n.icon;
    case ImageRole: return n.image;
    case Qt::DisplayRole:
    case SummaryRole: return n.summary;
    case BodyRole: return n.body;
    case ActionsRole: {
        QVariantList out;
        for (int i = 0; i + 1 < n.actions.size(); i += 2)
            out.append(QVariantMap{{QStringLiteral("key"), n.actions.at(i)}, {QStringLiteral("label"), n.actions.at(i + 1)}});
        return out;
    }
    case TimeRole: return n.time;
    case BannerRole: return n.banner;
    case UrgentRole: return n.urgent;
    default: return {};
    }
}

QHash<int, QByteArray> Notifications::roleNames() const
{
    return {{IdRole, "notificationId"}, {AppRole, "app"}, {IconRole, "icon"}, {ImageRole, "image"}, {SummaryRole, "summary"},
            {BodyRole, "body"}, {ActionsRole, "actions"}, {TimeRole, "time"}, {BannerRole, "banner"},
            {UrgentRole, "urgent"}};
}

void Notifications::setDnd(bool d)
{
    if (d == m_dnd)
        return;
    m_dnd = d;
    emit dndChanged();
    if (d) {
        // Banners showing now go quietly into the history.
        for (int i = 0; i < m_items.size(); ++i) {
            if (m_items[i].banner && !m_items[i].urgent) {
                m_items[i].banner = false;
                emit dataChanged(index(i), index(i), {BannerRole});
            }
        }
        emit countChanged();
    }
}

void Notifications::setBannerSeconds(int s)
{
    if (s == m_bannerSeconds)
        return;
    m_bannerSeconds = s;
    emit bannerSecondsChanged();
}

int Notifications::bannerCount() const
{
    return int(std::count_if(m_items.cbegin(), m_items.cend(), [](const Item &i) { return i.banner; }));
}

int Notifications::row(uint id) const
{
    for (int i = 0; i < m_items.size(); ++i)
        if (m_items.at(i).id == id)
            return i;
    return -1;
}

uint Notifications::add(Item item, uint replaces)
{
    item.time = QDateTime::currentDateTime();
    // Urgent ones get through Do Not Disturb (the spec's "critical").
    item.banner = !m_dnd || item.urgent;
    const int existing = replaces ? row(replaces) : -1;
    if (existing >= 0) {
        item.id = replaces;
        m_items[existing] = item;
        emit dataChanged(index(existing), index(existing));
    } else {
        item.id = ++m_lastId;
        beginInsertRows({}, 0, 0);
        m_items.prepend(item);
        endInsertRows();
    }
    if (item.banner) {
        // 0: until dismissed (also for urgent ones); otherwise the app's
        // time or the user's banner time.
        const qint64 ms = item.timeout == 0 || item.urgent ? 0
                          : item.timeout > 0              ? item.timeout
                                                          : qint64(m_bannerSeconds) * 1000;
        m_bannerUntil[item.id] = ms ? QDateTime::currentMSecsSinceEpoch() + ms : 0;
        m_tick.start();
    }
    // Never the text: what people are told stays out of the system log.
    qCInfo(lcShell) << "notification" << item.id << "from" << item.app << (item.banner ? "as a banner" : "quietly");
    emit countChanged();
    emit notified(int(item.id));
    return item.id;
}

void Notifications::expireBanners()
{
    const qint64 now = QDateTime::currentMSecsSinceEpoch();
    bool any = false;
    for (int i = 0; i < m_items.size(); ++i) {
        Item &n = m_items[i];
        if (!n.banner)
            continue;
        any = true;
        const qint64 until = m_bannerUntil.value(n.id);
        if (until && now >= until && !m_held.contains(n.id)) {
            n.banner = false;
            emit dataChanged(index(i), index(i), {BannerRole});
            emit countChanged();
        }
    }
    if (!any)
        m_tick.stop();
}

void Notifications::close(uint id, Reason reason)
{
    const int r = row(id);
    if (r < 0)
        return;
    beginRemoveRows({}, r, r);
    m_items.removeAt(r);
    endRemoveRows();
    m_bannerUntil.remove(id);
    m_held.remove(id);
    emit countChanged();
    emit closedSignal(id, uint(reason));
}

void Notifications::dismiss(int id)
{
    close(uint(id), Dismissed);
}

void Notifications::clearAll()
{
    const QList<Item> items = m_items;
    for (const Item &n : items)
        close(n.id, Dismissed);
}

void Notifications::invoke(int id, const QString &action)
{
    const int r = row(uint(id));
    if (r < 0)
        return;
    emit actionSignal(uint(id), action);
    if (!m_items.at(r).resident)
        close(uint(id), Dismissed);
}

void Notifications::hold(int id, bool held)
{
    if (held) {
        m_held.insert(uint(id));
    } else {
        m_held.remove(uint(id));
        // A short grace after the pointer leaves.
        if (m_bannerUntil.value(uint(id)))
            m_bannerUntil[uint(id)] = std::max(m_bannerUntil[uint(id)], QDateTime::currentMSecsSinceEpoch() + 1500);
    }
}

QString Notifications::localImage(const QString &pathOrUrl)
{
    const QUrl url = pathOrUrl.startsWith(QLatin1Char('/')) ? QUrl::fromLocalFile(pathOrUrl) : QUrl(pathOrUrl);
    if (!url.isLocalFile() || !QFileInfo(url.toLocalFile()).isFile())
        return {};
    return url.toString();
}

int Notifications::post(const QString &app, const QString &summary, const QString &body, const QString &image)
{
    Item item;
    item.app = app;
    item.summary = summary;
    item.body = body;
    item.image = localImage(image);
    return int(add(item, 0));
}
