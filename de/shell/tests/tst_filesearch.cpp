// FileSearch: where a result is, shown relative to home.
#include <QtTest>

#include "filesearch.h"

class TestFileSearch : public QObject
{
    Q_OBJECT
private slots:
    void where()
    {
        const QString home = QStringLiteral("/home/sam");
        QCOMPARE(FileSearch::where("/home/sam/Documents/a.txt", home), QStringLiteral("~/Documents"));
        QCOMPARE(FileSearch::where("/home/sam/a.txt", home), QStringLiteral("~"));
        QCOMPARE(FileSearch::where("/home/samuel/a.txt", home), QStringLiteral("/home/samuel"));
        QCOMPARE(FileSearch::where("/srv/x/a.txt", home), QStringLiteral("/srv/x"));
    }
    void noIndexer()
    {
        // Without the indexer on the bus: no results, no error.
        FileSearch f;
        f.setText(QStringLiteral("report"));
        QTest::qWait(300);
        QVERIFY(f.results().isEmpty());
    }
};

QTEST_GUILESS_MAIN(TestFileSearch)
#include "tst_filesearch.moc"
