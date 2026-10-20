// Media: the player to control from the control center, over MPRIS
// (org.mpris.MediaPlayer2.*): the one playing, else the last that did.
#pragma once

#include <QDBusVariant>
#include <QObject>
#include <QtQml/qqmlregistration.h>

class Media : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    Q_PROPERTY(bool available READ available NOTIFY changed)
    Q_PROPERTY(QString title READ title NOTIFY changed)
    Q_PROPERTY(QString artist READ artist NOTIFY changed)
    Q_PROPERTY(QString player READ player NOTIFY changed)
    Q_PROPERTY(bool playing READ playing NOTIFY changed)

public:
    explicit Media(QObject *parent = nullptr);

    bool available() const { return !m_service.isEmpty(); }
    QString title() const { return m_title; }
    QString artist() const { return m_artist; }
    QString player() const { return m_player; }
    bool playing() const { return m_playing; }

    Q_INVOKABLE void playPause() { call(QStringLiteral("PlayPause")); }
    Q_INVOKABLE void next() { call(QStringLiteral("Next")); }
    Q_INVOKABLE void previous() { call(QStringLiteral("Previous")); }

signals:
    void changed();

private slots:
    void ownerChanged(const QString &name, const QString &oldOwner, const QString &newOwner);
    void propertiesChanged(const QString &iface, const QVariantMap &changed, const QStringList &invalidated);

private:
    void scan();
    void choose();
    void read(const QString &service);
    void call(const QString &method);

    QStringList m_players;
    QString m_service, m_title, m_artist, m_player;
    bool m_playing = false;
};
