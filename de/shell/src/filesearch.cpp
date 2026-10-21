#include "filesearch.h"

#include <QDBusArgument>
#include <QDBusConnection>
#include <QDBusMessage>
#include <QDBusPendingCallWatcher>
#include <QDir>
#include <QFileInfo>

FileSearch::FileSearch(QObject *parent)
    : QObject(parent)
{
}

void FileSearch::setText(const QString &t)
{
    if (t == m_text)
        return;
    m_text = t;
    emit textChanged();
    query();
}

QString FileSearch::where(const QString &path, const QString &home)
{
    const QString dir = QFileInfo(path).path();
    if (dir == home)
        return QStringLiteral("~");
    if (dir.startsWith(home + QLatin1Char('/')))
        return QStringLiteral("~") + dir.mid(home.size());
    return dir;
}

void FileSearch::query()
{
    const quint64 id = ++m_sent;
    if (m_text.trimmed().isEmpty()) {
        if (!m_results.isEmpty()) {
            m_results.clear();
            emit resultsChanged();
        }
        return;
    }
    QDBusMessage call = QDBusMessage::createMethodCall(QStringLiteral("org.fern.Search"), QStringLiteral("/org/fern/Search"),
                                                       QStringLiteral("org.fern.Search1"), QStringLiteral("Query"));
    call << m_text << uint(m_limit);
    auto *w = new QDBusPendingCallWatcher(QDBusConnection::sessionBus().asyncCall(call, 2000), this);
    connect(w, &QDBusPendingCallWatcher::finished, this, [this, id](QDBusPendingCallWatcher *w) {
        w->deleteLater();
        // A newer query went out meanwhile: its answer counts.
        if (id != m_sent)
            return;
        QVariantList out;
        const QDBusMessage r = w->reply();
        if (r.type() == QDBusMessage::ReplyMessage && !r.arguments().isEmpty()) {
            const QString home = QDir::homePath();
            const QDBusArgument arg = r.arguments().constFirst().value<QDBusArgument>();
            arg.beginArray();
            while (!arg.atEnd()) {
                QString path, name;
                bool folder = false;
                arg.beginStructure();
                arg >> path >> name >> folder;
                arg.endStructure();
                out.append(QVariantMap{{QStringLiteral("path"), path},
                                       {QStringLiteral("name"), name},
                                       {QStringLiteral("folder"), folder},
                                       {QStringLiteral("where"), where(path, home)}});
            }
            arg.endArray();
        }
        // No indexer (yet): no files, and Search carries on.
        if (out != m_results) {
            m_results = out;
            emit resultsChanged();
        }
    });
}
