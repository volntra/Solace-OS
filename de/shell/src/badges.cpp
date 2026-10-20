#include "badges.h"

#include <QDBusConnection>

Badges::Badges(QObject *parent)
    : QObject(parent)
{
    QDBusConnection::sessionBus().connect(QString(), QString(), QStringLiteral("com.canonical.Unity.LauncherEntry"),
                                          QStringLiteral("Update"), this, SLOT(onUpdate(QString, QVariantMap)));
}

QString Badges::appIdFromUri(const QString &uri)
{
    QString id = uri;
    if (id.startsWith(QLatin1String("application://")))
        id = id.mid(14);
    if (id.endsWith(QLatin1String(".desktop")))
        id.chop(8);
    return id;
}

void Badges::onUpdate(const QString &uri, const QVariantMap &props)
{
    update(uri, props);
}

void Badges::update(const QString &uri, const QVariantMap &props)
{
    const QString id = appIdFromUri(uri);
    Entry &e = m_entries[id];
    for (auto it = props.cbegin(); it != props.cend(); ++it) {
        const QString &k = it.key();
        if (k == QLatin1String("count"))
            e.count = it.value().toLongLong();
        else if (k == QLatin1String("count-visible"))
            e.countVisible = it.value().toBool();
        else if (k == QLatin1String("progress"))
            e.progress = std::clamp(it.value().toDouble(), 0.0, 1.0);
        else if (k == QLatin1String("progress-visible"))
            e.progressVisible = it.value().toBool();
        else if (k == QLatin1String("urgent"))
            e.urgent = it.value().toBool();
    }
    emit changed(id);
}

QVariantMap Badges::get(const QString &appId) const
{
    const Entry e = m_entries.value(appId);
    return {{QStringLiteral("count"), e.countVisible ? e.count : 0},
            {QStringLiteral("progress"), e.progressVisible ? e.progress : -1.0},
            {QStringLiteral("urgent"), e.urgent}};
}
