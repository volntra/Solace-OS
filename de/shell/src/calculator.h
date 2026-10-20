// Calculator: what Search answers without looking anything up. Arithmetic
// (+ - * / ^ %, parentheses, sqrt sin cos tan ln log abs, pi, e) and unit
// conversion ("5 km in miles", "72 f to c", "3 GB in MB").
#pragma once

#include <QObject>
#include <QtQml/qqmlregistration.h>

class Calculator : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON

public:
    using QObject::QObject;

    // The answer as text ("= 42", "3.107 mi"), or empty when `text` is not
    // a sum or a conversion.
    Q_INVOKABLE static QString evaluate(const QString &text);

    // Pieces, public for tests. `ok` is false when the text is not a
    // complete expression.
    static double arithmetic(const QString &text, bool *ok);
    static QString convert(const QString &text);
    static QString format(double v);
};
