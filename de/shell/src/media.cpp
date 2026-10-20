#include "media.h"

#include <QDBusArgument>
#include <QDBusConnection>
#include <QDBusConnectionInterface>
#include <QDBusMessage>
#include <QDBusReply>

namespace {
const QString Prefix = QStringLiteral("org.mpris.MediaPlayer2.");
const QString Path = QStringLiteral("/org/mpris/MediaPlayer2");
const QString Player = QStringLiteral("org.mpris.MediaPlayer2.Player");

QVariant unwrap(QVariant v)
{
    if (v.canConvert<QDBusVariant>())
        v = v.value<QDBusVariant>().variant();
    return v;
}
}

Media::Media(QObject *parent)
    : QObject(parent)
{
    QDBusConnection bus = QDBusConnection::sessionBus();
    if (!bus.isConnected())
        return;
    bus.connect(QStringLiteral("org.freedesktop.DBus"), QStringLiteral("/org/freedesktop/DBus"),
                QStringLiteral("org.freedesktop.DBus"), QStringLiteral("NameOwnerChanged"), this,
                SLOT(ownerChanged(QString, QString, QString)));
    bus.connect(QString(), Path, QStringLiteral("org.freedesktop.DBus.Properties"), QStringLiteral("PropertiesChanged"),
                this, SLOT(propertiesChanged(QString, QVariantMap, QStringList)));
    scan();
}

void Media::scan()
{
    const QDBusReply<QStringList> names = QDBusConnection::sessionBus().interface()->registeredServiceNames();
    for (const QString &n : names.value())
        if (n.startsWith(Prefix) && !m_players.contains(n))
            m_players.append(n);
    choose();
}

void Media::ownerChanged(const QString &name, const QString &, const QString &newOwner)
{
    if (!name.startsWith(Prefix))
        return;
    m_players.removeAll(name);
    if (!newOwner.isEmpty())
        m_players.prepend(name);
    choose();
}

void Media::propertiesChanged(const QString &iface, const QVariantMap &, const QStringList &)
{
    if (iface == Player)
        choose();
}

void Media::choose()
{
    // The first one playing; else keep the current one if it is still
    // there; else the most recent.
    QString pick;
    for (const QString &s : std::as_const(m_players)) {
        QDBusMessage m = QDBusMessage::createMethodCall(s, Path, QStringLiteral("org.freedesktop.DBus.Properties"),
                                                        QStringLiteral("Get"));
        m << Player << QStringLiteral("PlaybackStatus");
        const QDBusMessage r = QDBusConnection::sessionBus().call(m, QDBus::Block, 300);
        if (!r.arguments().isEmpty() && unwrap(r.arguments().first()).toString() == QLatin1String("Playing")) {
            pick = s;
            break;
        }
    }
    if (pick.isEmpty())
        pick = m_players.contains(m_service) ? m_service : m_players.value(0);
    read(pick);
}

void Media::read(const QString &service)
{
    m_service = service;
    m_title.clear();
    m_artist.clear();
    m_player.clear();
    m_playing = false;
    if (!service.isEmpty()) {
        QDBusMessage m = QDBusMessage::createMethodCall(service, Path, QStringLiteral("org.freedesktop.DBus.Properties"),
                                                        QStringLiteral("GetAll"));
        m << Player;
        const QDBusMessage r = QDBusConnection::sessionBus().call(m, QDBus::Block, 300);
        if (!r.arguments().isEmpty()) {
            QVariantMap props;
            r.arguments().first().value<QDBusArgument>() >> props;
            m_playing = unwrap(props.value(QStringLiteral("PlaybackStatus"))).toString() == QLatin1String("Playing");
            QVariantMap meta;
            const QVariant md = unwrap(props.value(QStringLiteral("Metadata")));
            if (md.canConvert<QDBusArgument>())
                md.value<QDBusArgument>() >> meta;
            m_title = unwrap(meta.value(QStringLiteral("xesam:title"))).toString();
            const QVariant artist = unwrap(meta.value(QStringLiteral("xesam:artist")));
            m_artist = artist.canConvert<QStringList>() ? artist.toStringList().join(QStringLiteral(", ")) : artist.toString();
        }
        m_player = service.mid(Prefix.size()).section(QLatin1Char('.'), 0, 0);
    }
    emit changed();
}

void Media::call(const QString &method)
{
    if (m_service.isEmpty())
        return;
    QDBusConnection::sessionBus().asyncCall(QDBusMessage::createMethodCall(m_service, Path, Player, method));
}
