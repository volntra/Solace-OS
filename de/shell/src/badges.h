// Badges: what apps ask their dock icon to show, from the
// com.canonical.Unity.LauncherEntry Update signal: a count, a progress
// bar, and "urgent" (the icon bounces for attention).
#pragma once

#include <QHash>
#include <QObject>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

class Badges : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON

public:
    explicit Badges(QObject *parent = nullptr);

    // {count, progress, urgent} for an app id: count 0 and progress -1
    // when hidden.
    Q_INVOKABLE QVariantMap get(const QString &appId) const;

    // The app id an Update names: "application://org.example.App.desktop".
    static QString appIdFromUri(const QString &uri);
    // Applies one Update's properties (public for tests).
    void update(const QString &uri, const QVariantMap &props);

signals:
    void changed(const QString &appId);

private slots:
    void onUpdate(const QString &uri, const QVariantMap &props);

private:
    struct Entry
    {
        qint64 count = 0;
        bool countVisible = false;
        double progress = 0;
        bool progressVisible = false;
        bool urgent = false;
    };
    QHash<QString, Entry> m_entries;
};
