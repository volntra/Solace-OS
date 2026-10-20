// Night Shift: schedules (crossing midnight, sunset, off), warmth.
#include <QtTest>

#include "nightshift.h"

class TestNightShift : public QObject
{
    Q_OBJECT
private slots:
    void schedules()
    {
        const int h22 = 22 * 60, h7 = 7 * 60;
        // Across midnight.
        QVERIFY(NightShift::scheduled("custom", h22, h7, 23 * 60));
        QVERIFY(NightShift::scheduled("custom", h22, h7, 3 * 60));
        QVERIFY(NightShift::scheduled("custom", h22, h7, h22));
        QVERIFY(!NightShift::scheduled("custom", h22, h7, h7));
        QVERIFY(!NightShift::scheduled("custom", h22, h7, 12 * 60));
        // Within a day.
        QVERIFY(NightShift::scheduled("custom", 60, 120, 90));
        QVERIFY(!NightShift::scheduled("custom", 60, 120, 120));
        QVERIFY(!NightShift::scheduled("custom", 60, 60, 60));
        // Sunset: 19:00 to 7:00.
        QVERIFY(NightShift::scheduled("sunset", 0, 0, 20 * 60));
        QVERIFY(!NightShift::scheduled("sunset", 0, 0, 12 * 60));
        QVERIFY(!NightShift::scheduled("off", h22, h7, 23 * 60));
    }
    void warmth()
    {
        QCOMPARE(NightShift::kelvin(0), 6000);
        QCOMPARE(NightShift::kelvin(100), 2700);
        QCOMPARE(NightShift::kelvin(60), 4020);
        QCOMPARE(NightShift::kelvin(-5), 6000);
        QCOMPARE(NightShift::kelvin(500), 2700);
    }
};

QTEST_GUILESS_MAIN(TestNightShift)
#include "tst_nightshift.moc"
