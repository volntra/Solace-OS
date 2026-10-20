#include "devices.h"

#include <QDBusArgument>
#include <QDBusConnection>
#include <QDBusMessage>
#include <QDBusMetaType>
#include <QDBusObjectPath>
#include <QDBusPendingCall>
#include <QDBusVariant>
#include <QDir>
#include <QFile>
#include <QProcess>
#include <QRegularExpression>
#include <QStandardPaths>

namespace {
const QString NM = QStringLiteral("org.freedesktop.NetworkManager");
const QString NMPath = QStringLiteral("/org/freedesktop/NetworkManager");
const QString Props = QStringLiteral("org.freedesktop.DBus.Properties");

QVariant dbusGet(const QString &service, const QString &path, const QString &iface, const QString &name)
{
    QDBusMessage m = QDBusMessage::createMethodCall(service, path, Props, QStringLiteral("Get"));
    m << iface << name;
    const QDBusMessage r = QDBusConnection::systemBus().call(m, QDBus::Block, 500);
    if (r.type() != QDBusMessage::ReplyMessage || r.arguments().isEmpty())
        return {};
    return r.arguments().first().value<QDBusVariant>().variant();
}

void dbusSet(const QString &service, const QString &path, const QString &iface, const QString &name, const QVariant &v)
{
    QDBusMessage m = QDBusMessage::createMethodCall(service, path, Props, QStringLiteral("Set"));
    m << iface << name << QVariant::fromValue(QDBusVariant(v));
    QDBusConnection::systemBus().asyncCall(m);
}

QString wpctl()
{
    return QStandardPaths::findExecutable(QStringLiteral("wpctl"));
}
}

Devices::Devices(QObject *parent)
    : QObject(parent)
{
    const QDir bl(QStringLiteral("/sys/class/backlight"));
    const QStringList lights = bl.entryList(QDir::Dirs | QDir::NoDotAndDotDot | QDir::System);
    if (!lights.isEmpty()) {
        m_backlight = bl.filePath(lights.first());
        QFile max(m_backlight + QStringLiteral("/max_brightness"));
        if (max.open(QIODevice::ReadOnly))
            m_maxBrightness = std::max(1, max.readAll().trimmed().toInt());
    }
    QDBusConnection sys = QDBusConnection::systemBus();
    if (sys.isConnected()) {
        sys.connect(NM, NMPath, Props, QStringLiteral("PropertiesChanged"), this, SLOT(refresh()));
    }
    refresh();
}

void Devices::refresh()
{
    readWifi();
    readBluetooth();
    readSound();
    if (!m_backlight.isEmpty()) {
        QFile cur(m_backlight + QStringLiteral("/brightness"));
        if (cur.open(QIODevice::ReadOnly)) {
            m_brightness = cur.readAll().trimmed().toDouble() / m_maxBrightness;
            emit brightnessChanged();
        }
    }
}

void Devices::readWifi()
{
    const QVariant hw = dbusGet(NM, NMPath, NM, QStringLiteral("WirelessHardwareEnabled"));
    m_wifiAvailable = hw.isValid() && hw.toBool();
    m_wifiEnabled = dbusGet(NM, NMPath, NM, QStringLiteral("WirelessEnabled")).toBool();
    m_network.clear();
    const QVariant primary = dbusGet(NM, NMPath, NM, QStringLiteral("PrimaryConnection"));
    const QString path = primary.value<QDBusObjectPath>().path();
    if (!path.isEmpty() && path != QLatin1String("/"))
        m_network = dbusGet(NM, path, NM + QStringLiteral(".Connection.Active"), QStringLiteral("Id")).toString();
    emit wifiChanged();
}

void Devices::setWifiEnabled(bool on)
{
    if (!m_wifiAvailable)
        return;
    dbusSet(NM, NMPath, NM, QStringLiteral("WirelessEnabled"), on);
    m_wifiEnabled = on;
    emit wifiChanged();
}

void Devices::readBluetooth()
{
    m_adapter.clear();
    const QVariant powered = dbusGet(QStringLiteral("org.bluez"), QStringLiteral("/org/bluez/hci0"),
                                         QStringLiteral("org.bluez.Adapter1"), QStringLiteral("Powered"));
    if (powered.isValid()) {
        m_adapter = QStringLiteral("/org/bluez/hci0");
        m_btEnabled = powered.toBool();
    }
    emit bluetoothChanged();
}

void Devices::setBluetoothEnabled(bool on)
{
    if (m_adapter.isEmpty())
        return;
    dbusSet(QStringLiteral("org.bluez"), m_adapter, QStringLiteral("org.bluez.Adapter1"), QStringLiteral("Powered"), on);
    m_btEnabled = on;
    emit bluetoothChanged();
}

bool Devices::parseVolume(const QString &out, qreal *volume, bool *muted)
{
    static const QRegularExpression re(QStringLiteral("Volume:\\s*([0-9.]+)(\\s*\\[MUTED\\])?"));
    const auto m = re.match(out);
    if (!m.hasMatch())
        return false;
    *volume = m.captured(1).toDouble();
    *muted = m.capturedLength(2) > 0;
    return true;
}

void Devices::readSound()
{
    const QString tool = wpctl();
    m_soundAvailable = false;
    if (!tool.isEmpty()) {
        QProcess p;
        p.start(tool, {QStringLiteral("get-volume"), QStringLiteral("@DEFAULT_AUDIO_SINK@")});
        if (p.waitForFinished(500) && p.exitCode() == 0)
            m_soundAvailable = parseVolume(QString::fromUtf8(p.readAllStandardOutput()), &m_volume, &m_muted);
    }
    emit soundChanged();
}

void Devices::setVolume(qreal v)
{
    v = std::clamp(v, 0.0, 1.0);
    if (!m_soundAvailable || qFuzzyCompare(v + 1, m_volume + 1))
        return;
    m_volume = v;
    QProcess::startDetached(wpctl(), {QStringLiteral("set-volume"), QStringLiteral("@DEFAULT_AUDIO_SINK@"), QString::number(v, 'f', 3)});
    if (m_muted && v > 0)
        setMuted(false);
    emit soundChanged();
}

void Devices::setMuted(bool mute)
{
    if (!m_soundAvailable || mute == m_muted)
        return;
    m_muted = mute;
    QProcess::startDetached(wpctl(), {QStringLiteral("set-mute"), QStringLiteral("@DEFAULT_AUDIO_SINK@"), mute ? QStringLiteral("1") : QStringLiteral("0")});
    emit soundChanged();
}

void Devices::stepVolume(int direction)
{
    if (direction == 0)
        setMuted(!m_muted);
    else
        setVolume(std::round(m_volume * 16 + direction) / 16);
}

void Devices::setBrightness(qreal b)
{
    if (m_backlight.isEmpty())
        return;
    b = std::clamp(b, 0.02, 1.0);
    QFile f(m_backlight + QStringLiteral("/brightness"));
    // Writable when the session may (the login session owns it, Phase 9).
    if (f.open(QIODevice::WriteOnly)) {
        f.write(QByteArray::number(qRound(b * m_maxBrightness)));
        m_brightness = b;
        emit brightnessChanged();
    }
}
