// DBusMenu: a menu another program exports over com.canonical.dbusmenu
// (an app's menus for the global menu bar, a tray item's menu), read into
// the shape MenuPanel draws, and clicks sent back.
#pragma once

#include <QDBusArgument>
#include <QObject>
#include <QVariantList>
#include <QtQml/qqmlregistration.h>

class DBusMenu : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(QString service READ service WRITE setService NOTIFY addressChanged)
    Q_PROPERTY(QString path READ path WRITE setPath NOTIFY addressChanged)
    // The top-level entries: [{id, text, enabled, separator, checked,
    // shortcut, children: [...]}, ...].
    Q_PROPERTY(QVariantList items READ items NOTIFY itemsChanged)
    Q_PROPERTY(bool valid READ valid NOTIFY itemsChanged)

public:
    explicit DBusMenu(QObject *parent = nullptr);

    QString service() const { return m_service; }
    void setService(const QString &s);
    QString path() const { return m_path; }
    void setPath(const QString &p);
    QVariantList items() const { return m_items; }
    bool valid() const { return !m_items.isEmpty(); }

    // Tells the owner a submenu is about to open (it may fill it in), then
    // reads the menu again.
    Q_INVOKABLE void aboutToShow(int id);
    Q_INVOKABLE void activate(int id);
    Q_INVOKABLE void refresh();

    // Parsing, public for tests: one (ia{sv}av) layout node.
    static QVariantMap parseNode(const QDBusArgument &arg);
    // "_File" -> "File", "Save __As" -> "Save _As".
    static QString stripMnemonic(const QString &label);

signals:
    void addressChanged();
    void itemsChanged();

private slots:
    void layoutUpdated(uint revision, int parent);

private:
    static QVariantMap parseNode(const QDBusArgument &arg, int depth);
    void reconnect();

    QString m_service, m_path;
    QVariantList m_items;
};
