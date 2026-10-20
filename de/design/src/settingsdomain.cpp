#include "settingsdomain.h"

#include <QDBusArgument>
#include <QDBusConnection>
#include <QDBusMessage>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QDBusServiceWatcher>
#include <QDebug>

namespace {
const QString Service = QStringLiteral("org.fern.Settings");
const QString Path = QStringLiteral("/org/fern/Settings");
const QString Interface = QStringLiteral("org.fern.Settings1");
}

SettingsDomain::SettingsDomain(QObject *parent)
    : QQmlPropertyMap(this, parent)
{
}

void SettingsDomain::setDomain(const QString &d)
{
    if (d == m_domain)
        return;
    m_domain = d;
    emit domainChanged();
    if (m_complete)
        connectBus();
}

void SettingsDomain::setSystem(bool s)
{
    if (s == m_system)
        return;
    m_system = s;
    emit systemChanged();
    if (m_complete)
        connectBus();
}

void SettingsDomain::setDefaults(const QVariantMap &d)
{
    m_defaults = d;
    for (auto it = d.cbegin(); it != d.cend(); ++it)
        if (!m_ready || !contains(it.key()))
            insert(it.key(), it.value());
    emit defaultsChanged();
}

void SettingsDomain::componentComplete()
{
    m_complete = true;
    connectBus();
}

void SettingsDomain::connectBus()
{
    if (m_domain.isEmpty())
        return;
    QDBusConnection bus = m_system ? QDBusConnection::systemBus() : QDBusConnection::sessionBus();
    if (!bus.isConnected())
        return;
    // arg0 is the domain: only our domain's changes reach us.
    bus.connect(Service, Path, Interface, QStringLiteral("Changed"), {m_domain}, QString(), this,
                SLOT(onChanged(QString, QString, QDBusVariant)));
    if (!m_watcher) {
        // The daemon may start after us, or restart: read everything again.
        m_watcher = new QDBusServiceWatcher(Service, bus, QDBusServiceWatcher::WatchForRegistration, this);
        connect(m_watcher, &QDBusServiceWatcher::serviceRegistered, this, &SettingsDomain::fetch);
    }
    fetch();
}

void SettingsDomain::fetch()
{
    QDBusConnection bus = m_system ? QDBusConnection::systemBus() : QDBusConnection::sessionBus();
    QDBusMessage call = QDBusMessage::createMethodCall(Service, Path, Interface, QStringLiteral("GetAll"));
    call << m_domain;
    // Blocking on purpose, briefly: a component reads its settings before
    // it first draws, so it never flashes the defaults.
    QDBusMessage reply = bus.call(call, QDBus::Block, 1000);
    if (reply.type() != QDBusMessage::ReplyMessage || reply.arguments().isEmpty()) {
        if (reply.errorName() != QLatin1String("org.freedesktop.DBus.Error.ServiceUnknown"))
            qWarning() << "settings" << m_domain << reply.errorMessage();
        return;
    }
    const QDBusArgument arg = reply.arguments().constFirst().value<QDBusArgument>();
    QVariantMap all;
    arg >> all;
    for (auto it = all.cbegin(); it != all.cend(); ++it) {
        QVariant v = it.value();
        if (v.canConvert<QDBusVariant>())
            v = v.value<QDBusVariant>().variant();
        insert(it.key(), fromDBus(v));
    }
    if (!m_ready) {
        m_ready = true;
        emit readyChanged();
    }
}

void SettingsDomain::onChanged(const QString &domain, const QString &key, const QDBusVariant &value)
{
    if (domain != m_domain)
        return;
    const QVariant v = fromDBus(value.variant());
    if (this->value(key) == v)
        return;
    insert(key, v);
    emit valueChanged(key, v);
}

QVariant SettingsDomain::fromDBus(const QVariant &v)
{
    switch (v.typeId()) {
    case QMetaType::LongLong:
        return QVariant(int(v.toLongLong()));
    case QMetaType::ULongLong:
        return QVariant(int(v.toULongLong()));
    default:
        break;
    }
    if (v.canConvert<QDBusArgument>()) {
        // An `as` arrives demarshalled only as far as a QDBusArgument.
        QStringList list;
        v.value<QDBusArgument>() >> list;
        return list;
    }
    return v;
}

QVariant SettingsDomain::toDBus(const QVariant &input, const QVariant &current)
{
    if (current.typeId() == QMetaType::Int || current.typeId() == QMetaType::LongLong)
        return QVariant(qlonglong(input.toDouble()));
    if (current.typeId() == QMetaType::QStringList || input.canConvert<QVariantList>())
        if (input.typeId() != QMetaType::QString)
            return input.toStringList();
    if (input.typeId() == QMetaType::Bool)
        return input;
    if (input.typeId() == QMetaType::Double && current.typeId() == QMetaType::Bool)
        return QVariant(input.toDouble() != 0);
    return input;
}

QVariant SettingsDomain::updateValue(const QString &key, const QVariant &input)
{
    const QVariant current = value(key);
    const QVariant out = toDBus(input, current);
    if (!m_ready)
        return fromDBus(out);
    QDBusConnection bus = m_system ? QDBusConnection::systemBus() : QDBusConnection::sessionBus();
    QDBusMessage call = QDBusMessage::createMethodCall(Service, Path, Interface, QStringLiteral("Set"));
    call << m_domain << key << QVariant::fromValue(QDBusVariant(out));
    auto *watcher = new QDBusPendingCallWatcher(bus.asyncCall(call), this);
    connect(watcher, &QDBusPendingCallWatcher::finished, this, [this, key, current](QDBusPendingCallWatcher *w) {
        QDBusPendingReply<> r = *w;
        w->deleteLater();
        if (r.isError()) {
            // Refused (or no daemon): the old value comes back.
            m_error = r.error().message();
            emit errorChanged();
            insert(key, current);
            emit valueChanged(key, current);
        } else if (!m_error.isEmpty()) {
            m_error.clear();
            emit errorChanged();
        }
    });
    // Shown at once; the daemon's Changed confirms it.
    return fromDBus(out);
}

void SettingsDomain::reset(const QString &key)
{
    QDBusConnection bus = m_system ? QDBusConnection::systemBus() : QDBusConnection::sessionBus();
    QDBusMessage call = QDBusMessage::createMethodCall(Service, Path, Interface, QStringLiteral("Reset"));
    call << m_domain << key;
    bus.asyncCall(call);
}
