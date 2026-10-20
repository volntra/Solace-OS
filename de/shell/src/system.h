// System: what the Fern menu and About need: the computer's name and
// hardware, and restart / shut down (through fernctl). Sleep, Lock Screen
// and Log Out arrive with the login and lock screens (Phase 9).
#pragma once

#include <QObject>
#include <QtQml/qqmlregistration.h>

class System : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    Q_PROPERTY(QString hostName READ hostName CONSTANT)
    Q_PROPERTY(QString version READ version CONSTANT)
    Q_PROPERTY(QString processor READ processor CONSTANT)
    Q_PROPERTY(QString memory READ memory CONSTANT)
    Q_PROPERTY(QString graphics READ graphics CONSTANT)
    Q_PROPERTY(bool canPower READ canPower CONSTANT)
    Q_PROPERTY(bool canSleep READ canSleep CONSTANT)
    Q_PROPERTY(bool canLock READ canLock CONSTANT)
    Q_PROPERTY(QString home READ home CONSTANT)

public:
    explicit System(QObject *parent = nullptr);

    QString hostName() const;
    QString version() const { return m_version; }
    QString processor() const { return m_processor; }
    QString memory() const { return m_memory; }
    QString graphics() const { return m_graphics; }
    bool canPower() const;
    bool canSleep() const { return false; }
    bool canLock() const { return false; }

    QString home() const;
    // Deletes everything in the Trash (freedesktop Trash: files/ and
    // info/ under ~/.local/share/Trash).
    Q_INVOKABLE void emptyTrash();
    // A folder's entries, newest first: {name, size, modified, dir}.
    Q_INVOKABLE QVariantList listFolder(const QString &path, int limit = 50) const;
    Q_INVOKABLE void restart();
    Q_INVOKABLE void shutDown();

    // Parsers, public for tests.
    static QString osReleaseValue(const QString &text, const QString &key);
    static QString cpuModel(const QString &cpuinfo);
    static QString memoryTotal(const QString &meminfo);

private:
    QString m_version, m_processor, m_memory, m_graphics;
};
