// FileSearch: files and folders for Search, from fern-indexd
// (org.fern.Search1, docs/protocols/search.md). Queries go out as the user
// types; only the newest one's answer is shown.
#pragma once

#include <QObject>
#include <QVariantList>
#include <QtQml/qqmlregistration.h>

class FileSearch : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    // [{path, name, folder, where}, ...], best first; `where` is the
    // folder it is in, with the home folder as "~".
    Q_PROPERTY(QVariantList results READ results NOTIFY resultsChanged)
    Q_PROPERTY(QString text READ text WRITE setText NOTIFY textChanged)
    Q_PROPERTY(int limit MEMBER m_limit NOTIFY textChanged)

public:
    explicit FileSearch(QObject *parent = nullptr);

    QVariantList results() const { return m_results; }
    QString text() const { return m_text; }
    void setText(const QString &t);

    // "/home/sam/Documents/a.txt" -> "~/Documents"; public for tests.
    static QString where(const QString &path, const QString &home);

signals:
    void resultsChanged();
    void textChanged();

private:
    void query();

    QString m_text;
    QVariantList m_results;
    int m_limit = 5;
    quint64 m_sent = 0;
};
