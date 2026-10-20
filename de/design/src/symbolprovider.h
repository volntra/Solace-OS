// image://symbol/NAME?color=#rrggbb: a symbol (assets/symbols) drawn in a
// colour. Symbols are stroked with currentColor, which is replaced before
// rendering, so they stay crisp at every size.
#pragma once

#include <QQuickImageProvider>

class SymbolProvider : public QQuickImageProvider
{
public:
    SymbolProvider();
    QImage requestImage(const QString &id, QSize *size, const QSize &requestedSize) override;

    // Where the symbols are, in order: $FERN_SYMBOLS, the installed set,
    // the repository's (development: Lucide's and our own).
    static QStringList directories();
};
