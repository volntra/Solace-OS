#include "dbusmenu.h"

#include <QDBusConnection>
#include <QDBusMessage>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QDBusVariant>
#include <QDateTime>

namespace {
const QString Iface = QStringLiteral("com.canonical.dbusmenu");
// The menu comes from an app: bounded, so a hostile one cannot make the
// shell build a huge or endless menu.
constexpr int MaxDepth = 6;
constexpr int MaxChildren = 200;

QVariant unwrap(QVariant v)
{
    if (v.canConvert<QDBusVariant>())
        v = v.value<QDBusVariant>().variant();
    return v;
}

// A shortcut is a list of key lists: [["Control", "S"]].
QString shortcutText(const QVariant &v)
{
    QStringList keys;
    if (v.canConvert<QDBusArgument>()) {
        const QDBusArgument arg = v.value<QDBusArgument>();
        arg.beginArray();
        while (!arg.atEnd()) {
            QStringList combo;
            arg >> combo;
            for (QString &k : combo)
                if (k == QLatin1String("Control"))
                    k = QStringLiteral("Ctrl");
            keys.append(combo.join(QLatin1Char(' ')));
        }
        arg.endArray();
    }
    return keys.join(QStringLiteral(", "));
}
}

DBusMenu::DBusMenu(QObject *parent)
    : QObject(parent)
{
}

void DBusMenu::setService(const QString &s)
{
    if (s == m_service)
        return;
    m_service = s;
    emit addressChanged();
    reconnect();
}

void DBusMenu::setPath(const QString &p)
{
    if (p == m_path)
        return;
    m_path = p;
    emit addressChanged();
    reconnect();
}

void DBusMenu::reconnect()
{
    QDBusConnection bus = QDBusConnection::sessionBus();
    bus.disconnect(QString(), QString(), Iface, QStringLiteral("LayoutUpdated"), this, SLOT(layoutUpdated(uint, int)));
    if (m_service.isEmpty() || m_path.isEmpty()) {
        m_items.clear();
        emit itemsChanged();
        return;
    }
    bus.connect(m_service, m_path, Iface, QStringLiteral("LayoutUpdated"), this, SLOT(layoutUpdated(uint, int)));
    refresh();
}

void DBusMenu::layoutUpdated(uint, int)
{
    refresh();
}

QString DBusMenu::stripMnemonic(const QString &label)
{
    QString out;
    for (int i = 0; i < label.size(); ++i) {
        if (label.at(i) == QLatin1Char('_')) {
            if (i + 1 < label.size() && label.at(i + 1) == QLatin1Char('_'))
                out += QLatin1Char('_'), ++i;
            continue;
        }
        out += label.at(i);
    }
    return out;
}

QVariantMap DBusMenu::parseNode(const QDBusArgument &arg)
{
    return parseNode(arg, 0);
}

QVariantMap DBusMenu::parseNode(const QDBusArgument &arg, int depth)
{
    int id = 0;
    QVariantMap props;
    QVariantMap node;
    arg.beginStructure();
    arg >> id;
    arg >> props;
    QVariantList children;
    arg.beginArray();
    while (!arg.atEnd()) {
        QDBusVariant child;
        arg >> child;
        const QVariant v = child.variant();
        if (depth < MaxDepth && children.size() < MaxChildren && v.canConvert<QDBusArgument>()) {
            const QVariantMap c = parseNode(v.value<QDBusArgument>(), depth + 1);
            if (c.value(QStringLiteral("visible")).toBool())
                children.append(c);
        }
    }
    arg.endArray();
    arg.endStructure();

    node[QStringLiteral("id")] = id;
    node[QStringLiteral("text")] = stripMnemonic(unwrap(props.value(QStringLiteral("label"))).toString());
    const QVariant enabled = unwrap(props.value(QStringLiteral("enabled")));
    node[QStringLiteral("enabled")] = !enabled.isValid() || enabled.toBool();
    const QVariant visible = unwrap(props.value(QStringLiteral("visible")));
    node[QStringLiteral("visible")] = !visible.isValid() || visible.toBool();
    node[QStringLiteral("separator")] = unwrap(props.value(QStringLiteral("type"))).toString() == QLatin1String("separator");
    const QVariant state = unwrap(props.value(QStringLiteral("toggle-state")));
    node[QStringLiteral("checked")] = state.isValid() && state.toInt() == 1;
    node[QStringLiteral("shortcut")] = shortcutText(props.value(QStringLiteral("shortcut")));
    node[QStringLiteral("children")] = children;
    return node;
}

void DBusMenu::refresh()
{
    if (m_service.isEmpty() || m_path.isEmpty())
        return;
    QDBusMessage call = QDBusMessage::createMethodCall(m_service, m_path, Iface, QStringLiteral("GetLayout"));
    call << 0 << -1 << QStringList();
    auto *w = new QDBusPendingCallWatcher(QDBusConnection::sessionBus().asyncCall(call), this);
    connect(w, &QDBusPendingCallWatcher::finished, this, [this](QDBusPendingCallWatcher *w) {
        w->deleteLater();
        const QDBusMessage r = w->reply();
        if (r.type() != QDBusMessage::ReplyMessage || r.arguments().size() < 2) {
            m_items.clear();
            emit itemsChanged();
            return;
        }
        const QVariantMap root = parseNode(r.arguments().at(1).value<QDBusArgument>());
        m_items = root.value(QStringLiteral("children")).toList();
        emit itemsChanged();
    });
}

void DBusMenu::aboutToShow(int id)
{
    if (m_service.isEmpty())
        return;
    QDBusMessage call = QDBusMessage::createMethodCall(m_service, m_path, Iface, QStringLiteral("AboutToShow"));
    call << id;
    auto *w = new QDBusPendingCallWatcher(QDBusConnection::sessionBus().asyncCall(call), this);
    connect(w, &QDBusPendingCallWatcher::finished, this, [this](QDBusPendingCallWatcher *w) {
        w->deleteLater();
        QDBusPendingReply<bool> r = *w;
        if (r.isValid() && r.value())
            refresh();
    });
}

void DBusMenu::activate(int id)
{
    if (m_service.isEmpty())
        return;
    QDBusMessage call = QDBusMessage::createMethodCall(m_service, m_path, Iface, QStringLiteral("Event"));
    call << id << QStringLiteral("clicked") << QVariant::fromValue(QDBusVariant(QString()))
         << uint(QDateTime::currentSecsSinceEpoch());
    QDBusConnection::sessionBus().asyncCall(call);
}
