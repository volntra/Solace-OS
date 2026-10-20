// Tray: status icons apps put in the menu bar (the StatusNotifierItem
// specification). We are the session's org.kde.StatusNotifierWatcher and
// its host: apps register their items with us; we show each item's icon,
// activate it on click, and show its menu (dbusmenu) when it has one.
#pragma once

#include <QAbstractListModel>
#include <QDBusArgument>
#include <QDBusAbstractAdaptor>
#include <QDBusContext>
#include <QDBusMessage>
#include <QtQml/qqmlregistration.h>

class Tray;

class TrayWatcherAdaptor : public QDBusAbstractAdaptor
{
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.kde.StatusNotifierWatcher")
    Q_PROPERTY(QStringList RegisteredStatusNotifierItems READ items)
    Q_PROPERTY(bool IsStatusNotifierHostRegistered READ hostRegistered)
    Q_PROPERTY(int ProtocolVersion READ protocolVersion)
public:
    explicit TrayWatcherAdaptor(Tray *parent);
    QStringList items() const;
    bool hostRegistered() const { return true; }
    int protocolVersion() const { return 0; }

public slots:
    void RegisterStatusNotifierItem(const QString &service);
    void RegisterStatusNotifierHost(const QString &service);

signals:
    void StatusNotifierItemRegistered(const QString &service);
    void StatusNotifierItemUnregistered(const QString &service);
    void StatusNotifierHostRegistered();

private:
    Tray *m_tray;
};

class Tray : public QAbstractListModel, protected QDBusContext
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    Q_PROPERTY(int count READ rowCount NOTIFY countChanged)

public:
    enum Role { KeyRole = Qt::UserRole + 1, TitleRole, IconRole, StatusRole, ServiceRole, MenuRole, ItemIsMenuRole };

    struct Item
    {
        QString key; // "service/path"
        QString service, path;
        QString title, icon, status, menu;
        bool itemIsMenu = false;
    };

    explicit Tray(QObject *parent = nullptr);

    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

    Q_INVOKABLE void activate(const QString &key, int x, int y);

    // For the adaptor. `sender` is the caller's unique name.
    void registerItem(const QString &arg, const QString &sender);
    // The unique name of whoever made the call being handled.
    QString caller() const { return message().service(); }
    QStringList keys() const;

    // "org.app", "/path" (with the sender), or "org.app/path" into
    // (service, path); public for tests.
    static std::pair<QString, QString> address(const QString &arg, const QString &sender);
    // The largest icon pixmap (ARGB32, network byte order) up to 64 px,
    // as a data URL; empty when none.
    static QString pixmapUrl(const QDBusArgument &pixmaps);

signals:
    void countChanged();
    void itemRegistered(const QString &key);
    void itemUnregistered(const QString &key);

private slots:
    void ownerChanged(const QString &name, const QString &oldOwner, const QString &newOwner);
    void itemChanged();

private:
    void read(Item &item);
    int row(const QString &key) const;

    QList<Item> m_items;
};
