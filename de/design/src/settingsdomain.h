// SettingsDomain: one fern-settingsd domain as a QML object whose
// properties are the domain's keys (docs/protocols/settings.md).
//
//     SettingsDomain { id: dock; domain: "dock" }
//     ... dock.position ...        // "bottom"
//     dock.autohide = true         // written through the daemon
//
// Values arrive with the daemon's Changed signal. `defaults` (the schema's,
// for the keys a component uses) fill in until the daemon answers; without
// a daemon (tests, a broken session) writes stay local.
#pragma once

#include <QDBusVariant>
#include <QQmlParserStatus>
#include <QQmlPropertyMap>
#include <QtQml/qqmlregistration.h>

class QDBusServiceWatcher;

class SettingsDomain : public QQmlPropertyMap, public QQmlParserStatus
{
    Q_OBJECT
    Q_INTERFACES(QQmlParserStatus)
    QML_ELEMENT
    Q_PROPERTY(QString domain READ domain WRITE setDomain NOTIFY domainChanged)
    // The system's domains (boot) are on the system bus.
    Q_PROPERTY(bool system READ system WRITE setSystem NOTIFY systemChanged)
    // The daemon answered and every key has its value.
    Q_PROPERTY(bool ready READ ready NOTIFY readyChanged)
    // The last write the daemon refused, with its reason; empty when none.
    Q_PROPERTY(QString error READ error NOTIFY errorChanged)
    // Values to show before the daemon answers.
    Q_PROPERTY(QVariantMap defaults READ defaults WRITE setDefaults NOTIFY defaultsChanged)

public:
    explicit SettingsDomain(QObject *parent = nullptr);

    QString domain() const { return m_domain; }
    void setDomain(const QString &d);
    bool system() const { return m_system; }
    void setSystem(bool s);
    bool ready() const { return m_ready; }
    QString error() const { return m_error; }
    QVariantMap defaults() const { return m_defaults; }
    void setDefaults(const QVariantMap &d);

    // Back to the default.
    Q_INVOKABLE void reset(const QString &key);

    void classBegin() override {}
    void componentComplete() override;

    // D-Bus values as QML sees them: int64 becomes int, string lists stay.
    static QVariant fromDBus(const QVariant &v);
    // A QML value as the daemon wants it, given what the key holds now
    // (numbers become int64 for int keys).
    static QVariant toDBus(const QVariant &input, const QVariant &current);

signals:
    void domainChanged();
    void systemChanged();
    void readyChanged();
    void errorChanged();
    void defaultsChanged();

protected:
    QVariant updateValue(const QString &key, const QVariant &input) override;

private slots:
    void onChanged(const QString &domain, const QString &key, const QDBusVariant &value);

private:
    void connectBus();
    void fetch();

    QString m_domain;
    bool m_system = false;
    bool m_ready = false;
    bool m_complete = false;
    QString m_error;
    QVariantMap m_defaults;
    QDBusServiceWatcher *m_watcher = nullptr;
};
