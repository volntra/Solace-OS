// LauncherEntry badges: ids from URIs, partial updates, visibility.
#include <QtTest>

#include "badges.h"

class TestBadges : public QObject
{
    Q_OBJECT
private slots:
    void ids()
    {
        QCOMPARE(Badges::appIdFromUri(QStringLiteral("application://org.example.Mail.desktop")),
                 QStringLiteral("org.example.Mail"));
        QCOMPARE(Badges::appIdFromUri(QStringLiteral("org.example.Mail")), QStringLiteral("org.example.Mail"));
    }
    void updates()
    {
        Badges b;
        QSignalSpy spy(&b, &Badges::changed);
        const QString uri = QStringLiteral("application://org.example.Mail.desktop");
        b.update(uri, {{"count", 3}, {"count-visible", true}});
        QCOMPARE(spy.count(), 1);
        QCOMPARE(b.get("org.example.Mail").value("count").toLongLong(), 3);
        QCOMPARE(b.get("org.example.Mail").value("progress").toDouble(), -1.0);
        // Partial: only progress; the count stays.
        b.update(uri, {{"progress", 0.4}, {"progress-visible", true}});
        QCOMPARE(b.get("org.example.Mail").value("count").toLongLong(), 3);
        QCOMPARE(b.get("org.example.Mail").value("progress").toDouble(), 0.4);
        b.update(uri, {{"count-visible", false}, {"urgent", true}, {"progress", 7.0}});
        QCOMPARE(b.get("org.example.Mail").value("count").toLongLong(), 0);
        QVERIFY(b.get("org.example.Mail").value("urgent").toBool());
        QCOMPARE(b.get("org.example.Mail").value("progress").toDouble(), 1.0);
        QCOMPARE(b.get("org.other").value("count").toLongLong(), 0);
    }
};

QTEST_GUILESS_MAIN(TestBadges)
#include "tst_badges.moc"
