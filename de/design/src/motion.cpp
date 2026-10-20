#include "motion.h"

#include <cmath>
#include <numbers>

namespace {
// The duration is this many response times: by then the spring has come
// 99.9 % of the way (w t = 2 pi * 1.47 = 9.24).
constexpr double Span = 1.47;
constexpr double A = 2 * std::numbers::pi * Span;

double raw(double p)
{
    return 1.0 - (1.0 + A * p) * std::exp(-A * p);
}

double spring(qreal p)
{
    // Normalised so the animation ends exactly on its target.
    return raw(p) / raw(1.0);
}
}

QEasingCurve Motion::curve()
{
    QEasingCurve c;
    c.setCustomType(spring);
    return c;
}

int Motion::duration(double response)
{
    return int(std::lround(response * Span * 1000.0));
}

double Motion::sample(double p)
{
    return spring(p);
}
