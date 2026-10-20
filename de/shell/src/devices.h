// Devices: what the control center switches. Wi-Fi through
// NetworkManager, Bluetooth through BlueZ (both on the system bus), sound
// through WirePlumber's wpctl, brightness through the kernel's backlight.
// Each says whether it is available: on a system without the service (or
// the hardware) its tile says so instead of pretending.
#pragma once

#include <QObject>
#include <QTimer>
#include <QtQml/qqmlregistration.h>

class Devices : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    Q_PROPERTY(bool wifiAvailable READ wifiAvailable NOTIFY wifiChanged)
    Q_PROPERTY(bool wifiEnabled READ wifiEnabled WRITE setWifiEnabled NOTIFY wifiChanged)
    Q_PROPERTY(QString network READ network NOTIFY wifiChanged)
    Q_PROPERTY(bool bluetoothAvailable READ bluetoothAvailable NOTIFY bluetoothChanged)
    Q_PROPERTY(bool bluetoothEnabled READ bluetoothEnabled WRITE setBluetoothEnabled NOTIFY bluetoothChanged)
    Q_PROPERTY(bool soundAvailable READ soundAvailable NOTIFY soundChanged)
    Q_PROPERTY(qreal volume READ volume WRITE setVolume NOTIFY soundChanged)
    Q_PROPERTY(bool muted READ muted WRITE setMuted NOTIFY soundChanged)
    Q_PROPERTY(bool brightnessAvailable READ brightnessAvailable CONSTANT)
    Q_PROPERTY(qreal brightness READ brightness WRITE setBrightness NOTIFY brightnessChanged)

public:
    explicit Devices(QObject *parent = nullptr);

    bool wifiAvailable() const { return m_wifiAvailable; }
    bool wifiEnabled() const { return m_wifiEnabled; }
    void setWifiEnabled(bool on);
    QString network() const { return m_network; }
    bool bluetoothAvailable() const { return !m_adapter.isEmpty(); }
    bool bluetoothEnabled() const { return m_btEnabled; }
    void setBluetoothEnabled(bool on);
    bool soundAvailable() const { return m_soundAvailable; }
    qreal volume() const { return m_volume; }
    void setVolume(qreal v);
    bool muted() const { return m_muted; }
    void setMuted(bool m);
    bool brightnessAvailable() const { return !m_backlight.isEmpty(); }
    qreal brightness() const { return m_brightness; }
    void setBrightness(qreal b);

    // Read everything again (when the control center opens).
    Q_INVOKABLE void refresh();
    // Volume keys: one step up or down (1/16 of the range, like macOS).
    Q_INVOKABLE void stepVolume(int direction);
    Q_INVOKABLE void stepBrightness(int direction);

    // wpctl's "Volume: 0.40 [MUTED]", public for tests.
    static bool parseVolume(const QString &out, qreal *volume, bool *muted);

signals:
    void wifiChanged();
    void bluetoothChanged();
    void soundChanged();
    void brightnessChanged();

private:
    void readWifi();
    void readBluetooth();
    void readSound();

    bool m_wifiAvailable = false, m_wifiEnabled = false;
    QString m_network;
    QString m_adapter;
    bool m_btEnabled = false;
    bool m_soundAvailable = false, m_muted = false;
    qreal m_volume = 0;
    QString m_backlight;
    qreal m_brightness = 1;
    int m_maxBrightness = 1;
};
