// Search's calculator: sums, functions, conversions, and what it must not
// answer.
#include <QtTest>

#include "calculator.h"

class TestCalculator : public QObject
{
    Q_OBJECT
private slots:
    void arithmetic_data()
    {
        QTest::addColumn<QString>("text");
        QTest::addColumn<QString>("answer");
        QTest::newRow("sum") << "2+2" << "= 4";
        QTest::newRow("precedence") << "2 + 3 * 4" << "= 14";
        QTest::newRow("parentheses") << "(2 + 3) * 4" << "= 20";
        QTest::newRow("power right-assoc") << "2^3^2" << "= 512";
        QTest::newRow("unary") << "-3 * -3" << "= 9";
        QTest::newRow("division") << "10 / 4" << "= 2.5";
        QTest::newRow("modulo") << "17 % 5" << "= 2";
        QTest::newRow("grouping") << "1234567 * 1000" << "= 1,234,567,000";
        QTest::newRow("sqrt") << "sqrt(16) + 1" << "= 5";
        QTest::newRow("pi") << "2 * pi" << "= 6.283185307";
        QTest::newRow("exponent") << "1.5e3 / 3" << "= 500";
        QTest::newRow("times sign") << "6 × 7" << "= 42";
        QTest::newRow("no float noise") << "0.1 + 0.2" << "= 0.3";
    }
    void arithmetic()
    {
        QFETCH(QString, text);
        QFETCH(QString, answer);
        QCOMPARE(Calculator::evaluate(text), answer);
    }

    void conversions_data()
    {
        QTest::addColumn<QString>("text");
        QTest::addColumn<QString>("answer");
        QTest::newRow("km to mi") << "5 km in miles" << "3.106855961 mi";
        QTest::newRow("feet to m") << "10 ft to m" << "3.048 m";
        QTest::newRow("f to c") << "212 f to c" << "100 °C";
        QTest::newRow("c to f") << "-40 celsius in fahrenheit" << "-40 °F";
        QTest::newRow("k to c") << "0 k to c" << "-273.15 °C";
        QTest::newRow("gb to mb") << "3 GB in MB" << "3,000 MB";
        QTest::newRow("gib to mib") << "1 gib to mib" << "1,024 MiB";
        QTest::newRow("lb to kg") << "2.2 lb to kg" << "0.997903214 kg";
        QTest::newRow("hours to minutes") << "1.5 h in min" << "90 min";
    }
    void conversions()
    {
        QFETCH(QString, text);
        QFETCH(QString, answer);
        QCOMPARE(Calculator::evaluate(text), answer);
    }

    void not_answered_data()
    {
        QTest::addColumn<QString>("text");
        QTest::newRow("word") << "terminal";
        QTest::newRow("lone number") << "42";
        QTest::newRow("unbalanced") << "(2 + 3";
        QTest::newRow("trailing op") << "2 +";
        QTest::newRow("unknown function") << "foo(2)";
        QTest::newRow("mixed kinds") << "5 km to kg";
        QTest::newRow("division by zero") << "1/0";
        QTest::newRow("empty") << "";
    }
    void not_answered()
    {
        QFETCH(QString, text);
        QCOMPARE(Calculator::evaluate(text), QString());
    }
};

QTEST_APPLESS_MAIN(TestCalculator)
#include "tst_calculator.moc"
