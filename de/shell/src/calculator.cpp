#include "calculator.h"

#include <QHash>
#include <QLocale>
#include <QRegularExpression>
#include <cmath>
#include <numbers>

namespace {

// Recursive descent over:
//   expr   := term (('+' | '-') term)*
//   term   := power ((* | / | %) power)*
//   power  := unary ('^' power)?
//   unary  := '-' unary | '+' unary | call
//   call   := name '(' expr ')' | name | number | '(' expr ')'
class Parser
{
public:
    explicit Parser(const QString &s) : m_s(s) {}

    double parse(bool *ok)
    {
        m_ok = true;
        double v = expr();
        skip();
        if (m_i != m_s.size())
            m_ok = false;
        *ok = m_ok && std::isfinite(v);
        return v;
    }

private:
    void skip()
    {
        while (m_i < m_s.size() && m_s.at(m_i).isSpace())
            ++m_i;
    }
    bool eat(QChar c)
    {
        skip();
        if (m_i < m_s.size() && m_s.at(m_i) == c) {
            ++m_i;
            return true;
        }
        return false;
    }
    double expr()
    {
        double v = term();
        for (;;) {
            if (eat(QLatin1Char('+')))
                v += term();
            else if (eat(QLatin1Char('-')))
                v -= term();
            else
                return v;
        }
    }
    double term()
    {
        double v = power();
        for (;;) {
            if (eat(QLatin1Char('*')) || eat(QChar(0x00D7)))
                v *= power();
            else if (eat(QLatin1Char('/')) || eat(QChar(0x00F7)))
                v /= power();
            else if (eat(QLatin1Char('%')))
                v = std::fmod(v, power());
            else
                return v;
        }
    }
    double power()
    {
        double base = unary();
        if (eat(QLatin1Char('^')))
            return std::pow(base, power());
        return base;
    }
    double unary()
    {
        if (eat(QLatin1Char('-')))
            return -unary();
        if (eat(QLatin1Char('+')))
            return unary();
        return call();
    }
    double call()
    {
        skip();
        if (eat(QLatin1Char('('))) {
            double v = expr();
            if (!eat(QLatin1Char(')')))
                m_ok = false;
            return v;
        }
        if (m_i < m_s.size() && (m_s.at(m_i).isDigit() || m_s.at(m_i) == QLatin1Char('.'))) {
            const int start = m_i;
            while (m_i < m_s.size() && (m_s.at(m_i).isDigit() || m_s.at(m_i) == QLatin1Char('.')))
                ++m_i;
            // Exponent: 1e3, 2.5E-2.
            if (m_i < m_s.size() && (m_s.at(m_i) == QLatin1Char('e') || m_s.at(m_i) == QLatin1Char('E'))) {
                int j = m_i + 1;
                if (j < m_s.size() && (m_s.at(j) == QLatin1Char('+') || m_s.at(j) == QLatin1Char('-')))
                    ++j;
                if (j < m_s.size() && m_s.at(j).isDigit()) {
                    m_i = j;
                    while (m_i < m_s.size() && m_s.at(m_i).isDigit())
                        ++m_i;
                }
            }
            bool ok = false;
            double v = m_s.mid(start, m_i - start).toDouble(&ok);
            if (!ok)
                m_ok = false;
            return v;
        }
        const int start = m_i;
        while (m_i < m_s.size() && m_s.at(m_i).isLetter())
            ++m_i;
        const QString name = m_s.mid(start, m_i - start).toLower();
        if (name == QLatin1String("pi") || name == QStringLiteral("π"))
            return std::numbers::pi;
        if (name == QLatin1String("e"))
            return std::numbers::e;
        static const QHash<QString, double (*)(double)> fns = {
            {QStringLiteral("sqrt"), [](double x) { return std::sqrt(x); }},
            {QStringLiteral("sin"), [](double x) { return std::sin(x); }},
            {QStringLiteral("cos"), [](double x) { return std::cos(x); }},
            {QStringLiteral("tan"), [](double x) { return std::tan(x); }},
            {QStringLiteral("ln"), [](double x) { return std::log(x); }},
            {QStringLiteral("log"), [](double x) { return std::log10(x); }},
            {QStringLiteral("abs"), [](double x) { return std::fabs(x); }},
        };
        if (auto f = fns.value(name); f && eat(QLatin1Char('('))) {
            double v = expr();
            if (!eat(QLatin1Char(')')))
                m_ok = false;
            return f(v);
        }
        m_ok = false;
        return 0;
    }

    QString m_s;
    int m_i = 0;
    bool m_ok = true;
};

struct Unit
{
    const char *kind;
    double factor; // to the kind's base unit
    const char *symbol;
};

const QHash<QString, Unit> &units()
{
    static const QHash<QString, Unit> u = [] {
        QHash<QString, Unit> h;
        auto add = [&h](std::initializer_list<const char *> names, Unit unit) {
            for (const char *n : names)
                h.insert(QString::fromLatin1(n), unit);
        };
        // Length, base metre.
        add({"mm", "millimetre", "millimetres", "millimeter", "millimeters"}, {"length", 0.001, "mm"});
        add({"cm", "centimetre", "centimetres", "centimeter", "centimeters"}, {"length", 0.01, "cm"});
        add({"m", "metre", "metres", "meter", "meters"}, {"length", 1, "m"});
        add({"km", "kilometre", "kilometres", "kilometer", "kilometers"}, {"length", 1000, "km"});
        add({"in", "inch", "inches"}, {"length", 0.0254, "in"});
        add({"ft", "foot", "feet"}, {"length", 0.3048, "ft"});
        add({"yd", "yard", "yards"}, {"length", 0.9144, "yd"});
        add({"mi", "mile", "miles"}, {"length", 1609.344, "mi"});
        // Mass, base gram.
        add({"g", "gram", "grams"}, {"mass", 1, "g"});
        add({"kg", "kilogram", "kilograms"}, {"mass", 1000, "kg"});
        add({"t", "tonne", "tonnes"}, {"mass", 1e6, "t"});
        add({"oz", "ounce", "ounces"}, {"mass", 28.349523125, "oz"});
        add({"lb", "lbs", "pound", "pounds"}, {"mass", 453.59237, "lb"});
        // Volume, base litre.
        add({"ml", "millilitre", "millilitres", "milliliter", "milliliters"}, {"volume", 0.001, "ml"});
        add({"l", "litre", "litres", "liter", "liters"}, {"volume", 1, "l"});
        add({"gal", "gallon", "gallons"}, {"volume", 3.785411784, "gal"});
        add({"cup", "cups"}, {"volume", 0.2365882365, "cups"});
        // Time, base second.
        add({"s", "sec", "second", "seconds"}, {"time", 1, "s"});
        add({"min", "minute", "minutes"}, {"time", 60, "min"});
        add({"h", "hr", "hour", "hours"}, {"time", 3600, "h"});
        add({"day", "days"}, {"time", 86400, "days"});
        add({"week", "weeks"}, {"time", 604800, "weeks"});
        // Data, base byte.
        add({"b", "byte", "bytes"}, {"data", 1, "B"});
        add({"kb"}, {"data", 1e3, "KB"});
        add({"mb"}, {"data", 1e6, "MB"});
        add({"gb"}, {"data", 1e9, "GB"});
        add({"tb"}, {"data", 1e12, "TB"});
        add({"kib"}, {"data", 1024.0, "KiB"});
        add({"mib"}, {"data", 1048576.0, "MiB"});
        add({"gib"}, {"data", 1073741824.0, "GiB"});
        add({"tib"}, {"data", 1099511627776.0, "TiB"});
        // Temperature: handled apart (offsets), factor unused.
        add({"c", "°c", "celsius"}, {"temp", 0, "°C"});
        add({"f", "°f", "fahrenheit"}, {"temp", 0, "°F"});
        add({"k", "kelvin"}, {"temp", 0, "K"});
        return h;
    }();
    return u;
}

double toKelvin(double v, const QString &sym)
{
    if (sym == QStringLiteral("°C"))
        return v + 273.15;
    if (sym == QStringLiteral("°F"))
        return (v - 32) * 5 / 9 + 273.15;
    return v;
}

double fromKelvin(double v, const QString &sym)
{
    if (sym == QStringLiteral("°C"))
        return v - 273.15;
    if (sym == QStringLiteral("°F"))
        return (v - 273.15) * 9 / 5 + 32;
    return v;
}

} // namespace

double Calculator::arithmetic(const QString &text, bool *ok)
{
    Parser p(text);
    return p.parse(ok);
}

QString Calculator::format(double v)
{
    if (std::fabs(v) >= 1e15 || (v != 0 && std::fabs(v) < 1e-6))
        return QString::number(v, 'g', 10);
    // At most 10 significant digits, no trailing zeros, grouped.
    QString s = QLocale(QLocale::English).toString(v, 'f', std::max(0, 9 - int(std::floor(std::log10(std::fabs(v) + 1e-300)))));
    if (s.contains(QLatin1Char('.'))) {
        while (s.endsWith(QLatin1Char('0')))
            s.chop(1);
        if (s.endsWith(QLatin1Char('.')))
            s.chop(1);
    }
    return s == QLatin1String("-0") ? QStringLiteral("0") : s;
}

QString Calculator::convert(const QString &text)
{
    static const QRegularExpression re(
        QStringLiteral("^\\s*(-?[0-9.]+(?:[eE][-+]?[0-9]+)?)\\s*([a-zA-Z°]+)\\s+(?:to|in|as)\\s+([a-zA-Z°]+)\\s*$"));
    const auto m = re.match(text);
    if (!m.hasMatch())
        return {};
    bool ok = false;
    const double v = m.captured(1).toDouble(&ok);
    const auto &u = units();
    const auto from = u.find(m.captured(2).toLower());
    const auto to = u.find(m.captured(3).toLower());
    if (!ok || from == u.end() || to == u.end() || qstrcmp(from->kind, to->kind) != 0)
        return {};
    const QString toSym = QString::fromUtf8(to->symbol);
    double out;
    if (qstrcmp(from->kind, "temp") == 0)
        out = fromKelvin(toKelvin(v, QString::fromUtf8(from->symbol)), toSym);
    else
        out = v * from->factor / to->factor;
    return format(out) + QLatin1Char(' ') + toSym;
}

QString Calculator::evaluate(const QString &text)
{
    const QString t = text.trimmed();
    if (t.isEmpty())
        return {};
    if (QString c = convert(t); !c.isEmpty())
        return c;
    // A lone number is not a sum worth answering.
    static const QRegularExpression plain(QStringLiteral("^[-+]?[0-9.]+$"));
    if (plain.match(t).hasMatch())
        return {};
    bool ok = false;
    const double v = arithmetic(t, &ok);
    if (!ok)
        return {};
    return QStringLiteral("= ") + format(v);
}
