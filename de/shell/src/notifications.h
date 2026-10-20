// Notifications: our org.freedesktop.Notifications server (Desktop
// Notifications Specification 1.2). Every notification goes into the
// history; while Do Not Disturb is off, a new one also shows as a banner
// for the user's banner time (or until dismissed, when it asks to stay).
#pragma once

#include <QAbstractListModel>
#include <QDBusAbstractAdaptor>
#include <QDBusVariant>
#include <QDateTime>
#include <QTimer>
#include <QtQml/qqmlregistration.h>

class Notifications;

class NotificationsAdaptor : public QDBusAbstractAdaptor
{
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.freedesktop.Notifications")
public:
    explicit NotificationsAdaptor(Notifications *parent);

public slots:
    QStringList GetCapabilities();
    uint Notify(const QString &app_name, uint replaces_id, const QString &app_icon, const QString &summary,
                const QString &body, const QStringList &actions, const QVariantMap &hints, int expire_timeout);
    void CloseNotification(uint id);
    QString GetServerInformation(QString &vendor, QString &version, QString &spec_version);

signals:
    void NotificationClosed(uint id, uint reason);
    void ActionInvoked(uint id, const QString &action_key);

private:
    Notifications *m_n;
};

class Notifications : public QAbstractListModel
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    Q_PROPERTY(bool dnd READ dnd WRITE setDnd NOTIFY dndChanged)
    Q_PROPERTY(int bannerSeconds READ bannerSeconds WRITE setBannerSeconds NOTIFY bannerSecondsChanged)
    Q_PROPERTY(int count READ rowCount NOTIFY countChanged)
    Q_PROPERTY(int bannerCount READ bannerCount NOTIFY countChanged)
    // We own the name (another notification server was not running).
    Q_PROPERTY(bool serving READ serving CONSTANT)

public:
    enum Role { IdRole = Qt::UserRole + 1, AppRole, IconRole, SummaryRole, BodyRole, ActionsRole, TimeRole, BannerRole, UrgentRole };
    enum Reason { Expired = 1, Dismissed = 2, Closed = 3 };

    struct Item
    {
        uint id = 0;
        QString app, icon, summary, body;
        QStringList actions; // key, label, key, label...
        QDateTime time;
        bool banner = false;
        bool urgent = false;
        bool resident = false;
        int timeout = -1; // ms; 0 never, -1 the user's banner time
    };

    explicit Notifications(QObject *parent = nullptr);

    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

    bool dnd() const { return m_dnd; }
    void setDnd(bool d);
    int bannerSeconds() const { return m_bannerSeconds; }
    void setBannerSeconds(int s);
    int bannerCount() const;
    bool serving() const { return m_serving; }

    uint add(Item item, uint replaces);
    void close(uint id, Reason reason);

    // From QML: the user dismissed one / all, or clicked an action (the
    // body is the "default" action).
    Q_INVOKABLE void dismiss(int id);
    Q_INVOKABLE void clearAll();
    Q_INVOKABLE void invoke(int id, const QString &action);
    // The banner stays while the pointer is on it.
    Q_INVOKABLE void hold(int id, bool held);

signals:
    void dndChanged();
    void bannerSecondsChanged();
    void countChanged();
    void notified(int id);
    void closedSignal(uint id, uint reason);
    void actionSignal(uint id, const QString &key);

private:
    int row(uint id) const;
    void expireBanners();

    QList<Item> m_items; // newest first
    QHash<uint, qint64> m_bannerUntil;
    QSet<uint> m_held;
    QTimer m_tick;
    uint m_lastId = 0;
    bool m_dnd = false;
    int m_bannerSeconds = 5;
    bool m_serving = false;
};
