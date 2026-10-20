#include "nightshift.h"

#include <QTime>

#include <algorithm>

bool NightShift::scheduled(const QString &schedule, int from, int to, int minutes)
{
    if (schedule == QLatin1String("sunset")) {
        from = 19 * 60;
        to = 7 * 60;
    } else if (schedule != QLatin1String("custom")) {
        return false;
    }
    if (from == to)
        return false;
    if (from < to)
        return minutes >= from && minutes < to;
    return minutes >= from || minutes < to;
}

int NightShift::kelvin(int warmth)
{
    warmth = std::clamp(warmth, 0, 100);
    return 6000 - (6000 - 2700) * warmth / 100;
}

int NightShift::minutesNow()
{
    const QTime t = QTime::currentTime();
    return t.hour() * 60 + t.minute();
}
