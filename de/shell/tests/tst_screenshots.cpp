// Screenshots: names, never overwriting, saving.
#include <QtTest>

#include "screenshots.h"

class TestScreenshots : public QObject
{
    Q_OBJECT
private slots:
    void names()
    {
        const QDateTime when(QDate(2026, 10, 20), QTime(21, 4, 7));
        QCOMPARE(Screenshots::fileName(when), QStringLiteral("Screenshot 2026-10-20 at 21.04.07.png"));
    }
    void saving()
    {
        QTemporaryDir tmp;
        const QString dir = tmp.filePath(QStringLiteral("Pictures/Screenshots"));
        const QDateTime when(QDate(2026, 10, 20), QTime(9, 0, 0));
        QImage image(40, 30, QImage::Format_RGB32);
        image.fill(QColor(10, 200, 30));
        const QString first = Screenshots::save(image, dir, when);
        QCOMPARE(QFileInfo(first).fileName(), QStringLiteral("Screenshot 2026-10-20 at 09.00.00.png"));
        // The same second again: a new name, the first kept.
        const QString second = Screenshots::save(image, dir, when);
        QCOMPARE(QFileInfo(second).fileName(), QStringLiteral("Screenshot 2026-10-20 at 09.00.00 2.png"));
        const QImage back(first);
        QCOMPARE(back.size(), QSize(40, 30));
        QCOMPARE(back.pixelColor(5, 5), QColor(10, 200, 30));
        QVERIFY(Screenshots::save(QImage(), dir, when).isEmpty());
    }
};

QTEST_GUILESS_MAIN(TestScreenshots)
#include "tst_screenshots.moc"
