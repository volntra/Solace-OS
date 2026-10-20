// Motion: the HIG's springs for QML (docs/design/HIG.md, Motion).
//
// Every spring is critically damped, so from rest its path does not depend
// on anything but its response time R:
//     x(t) = 1 - (1 + w t) e^(-w t),   w = 2 pi / R
// That is one curve stretched in time. `curve` is it as an easing curve,
// `snappy`/`smooth`/`gentle` the durations that reach 99.9 % of the way, so
//     Behavior on x { NumberAnimation { duration: Motion.smooth; easing: Motion.curve } }
// moves exactly like the compositor's windows (de/comp/src/spring.rs).
#pragma once

#include <QEasingCurve>
#include <QObject>
#include <QtQml/qqmlregistration.h>

class Motion : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    Q_PROPERTY(QEasingCurve curve READ curve CONSTANT)
    // Response times, as in the compositor: 0.25 s, 0.42 s, 0.6 s.
    Q_PROPERTY(int snappy READ snappy CONSTANT)
    Q_PROPERTY(int smooth READ smooth CONSTANT)
    Q_PROPERTY(int gentle READ gentle CONSTANT)

public:
    using QObject::QObject;

    static QEasingCurve curve();
    static int duration(double response);
    int snappy() const { return duration(0.25); }
    int smooth() const { return duration(0.42); }
    int gentle() const { return duration(0.6); }

    // The curve at progress p (0..1), for tests.
    Q_INVOKABLE static double sample(double p);
};
