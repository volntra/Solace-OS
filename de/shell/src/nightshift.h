// Night Shift's schedule and warmth (CLAUDE.md 6.8): when it is on, and
// how warm, from the user's nightshift settings. The compositor makes the
// colours warm (fern_shell_v1.set_color_temperature).
#pragma once

#include <QObject>
#include <QtQml/qqmlregistration.h>

class NightShift : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON

public:
    using QObject::QObject;

    // Whether the schedule ("off", "sunset", "custom") has it on at
    // `minutes` after midnight. "sunset" is 19:00 to 7:00 until location
    // services give real sunsets. A custom range may cross midnight;
    // from == to is never.
    Q_INVOKABLE static bool scheduled(const QString &schedule, int from, int to, int minutes);
    // The colour temperature for a warmth of 0..100: 6000 K (barely) to
    // 2700 K (candlelight).
    Q_INVOKABLE static int kelvin(int warmth);
    // Minutes after midnight now.
    Q_INVOKABLE static int minutesNow();
};
