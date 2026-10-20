#include "apps.h"

#include <QDir>
#include <QFile>
#include <QLocale>
#include <QProcess>
#include <QStandardPaths>
#include <QUrl>

Apps::Apps(QObject *parent)
    : QAbstractListModel(parent)
{
    const QString home = qEnvironmentVariable("XDG_DATA_HOME", QDir::homePath() + QStringLiteral("/.local/share"));
    m_dataDirs = {home};
    const QString dirs = qEnvironmentVariable("XDG_DATA_DIRS", QStringLiteral("/usr/local/share:/usr/share"));
    for (const QString &d : dirs.split(QLatin1Char(':'), Qt::SkipEmptyParts))
        if (!m_dataDirs.contains(d))
            m_dataDirs.append(d);
    connect(&m_watcher, &QFileSystemWatcher::directoryChanged, this, &Apps::rescan);
    rescan();
}

QStringList Apps::appDirs() const
{
    QStringList out;
    for (const QString &d : m_dataDirs)
        out.append(d + QStringLiteral("/applications"));
    return out;
}

void Apps::rescan()
{
    const QStringList dirs = appDirs();
    if (!m_watcher.directories().isEmpty())
        m_watcher.removePaths(m_watcher.directories());
    for (const QString &d : dirs)
        if (QDir(d).exists())
            m_watcher.addPath(d);
    loadFrom(dirs);
}

void Apps::loadFrom(const QStringList &dirs)
{
    const QString locale = QLocale().name();
    QList<DesktopEntry> apps;
    QStringList seen;
    // Earlier directories win (the user's own entries override the
    // system's).
    for (const QString &dir : dirs) {
        const QDir d(dir);
        for (const QFileInfo &fi : d.entryInfoList({QStringLiteral("*.desktop")}, QDir::Files, QDir::Name)) {
            const QString id = fi.completeBaseName();
            if (seen.contains(id))
                continue;
            seen.append(id);
            QFile f(fi.filePath());
            if (!f.open(QIODevice::ReadOnly))
                continue;
            DesktopEntry e = parse(id, f.readAll(), locale);
            if (e.name.isEmpty() || e.exec.isEmpty())
                continue;
            e.icon = findIcon(e.icon, m_dataDirs);
            apps.append(e);
        }
    }
    std::sort(apps.begin(), apps.end(), [](const DesktopEntry &a, const DesktopEntry &b) {
        return a.name.localeAwareCompare(b.name) < 0;
    });
    beginResetModel();
    m_apps = apps;
    endResetModel();
    emit countChanged();
}

DesktopEntry Apps::parse(const QString &id, const QByteArray &data, const QString &locale)
{
    DesktopEntry e;
    e.id = id;
    bool inEntry = false;
    const QString lang = locale.section(QLatin1Char('_'), 0, 0);
    // Localised values beat plain ones: exact locale, then language.
    QHash<QString, int> rank;
    auto set = [&](const QString &key, const QString &loc, const QString &value) {
        int r = loc.isEmpty() ? 0 : loc == locale ? 2 : loc == lang ? 1 : -1;
        if (r < 0 || r < rank.value(key, -1))
            return;
        rank[key] = r;
        if (key == QLatin1String("Name")) e.name = value;
        else if (key == QLatin1String("GenericName")) e.genericName = value;
        else if (key == QLatin1String("Comment")) e.comment = value;
        else if (key == QLatin1String("Keywords")) e.keywords = value.split(QLatin1Char(';'), Qt::SkipEmptyParts);
    };
    QString type;
    for (const QByteArray &raw : data.split('\n')) {
        const QString line = QString::fromUtf8(raw).trimmed();
        if (line.isEmpty() || line.startsWith(QLatin1Char('#')))
            continue;
        if (line.startsWith(QLatin1Char('['))) {
            inEntry = line == QLatin1String("[Desktop Entry]");
            continue;
        }
        if (!inEntry)
            continue;
        const int eq = line.indexOf(QLatin1Char('='));
        if (eq < 0)
            continue;
        QString key = line.left(eq).trimmed();
        const QString value = line.mid(eq + 1).trimmed();
        QString loc;
        if (const int b = key.indexOf(QLatin1Char('[')); b > 0 && key.endsWith(QLatin1Char(']'))) {
            loc = key.mid(b + 1, key.size() - b - 2);
            key = key.left(b);
        }
        if (key == QLatin1String("Name") || key == QLatin1String("GenericName") || key == QLatin1String("Comment")
            || key == QLatin1String("Keywords"))
            set(key, loc, value);
        else if (!loc.isEmpty())
            continue;
        else if (key == QLatin1String("Exec")) e.exec = value;
        else if (key == QLatin1String("Icon")) e.icon = value;
        else if (key == QLatin1String("StartupWMClass")) e.wmClass = value;
        else if (key == QLatin1String("Categories")) e.categories = value.split(QLatin1Char(';'), Qt::SkipEmptyParts);
        else if (key == QLatin1String("Terminal")) e.terminal = value == QLatin1String("true");
        else if (key == QLatin1String("Type")) type = value;
        else if (key == QLatin1String("NoDisplay") || key == QLatin1String("Hidden"))
            e.hidden = e.hidden || value == QLatin1String("true");
    }
    if (type != QLatin1String("Application"))
        e.exec.clear();
    return e;
}

QStringList Apps::execArgs(const QString &exec)
{
    // The Exec key's quoting rules (Desktop Entry spec, "The Exec key"),
    // with the field codes dropped: we never pass files from here.
    QStringList args;
    QString cur;
    bool quoted = false, any = false;
    for (int i = 0; i < exec.size(); ++i) {
        const QChar c = exec.at(i);
        if (quoted) {
            if (c == QLatin1Char('\\') && i + 1 < exec.size()) {
                cur += exec.at(++i);
            } else if (c == QLatin1Char('"')) {
                quoted = false;
            } else {
                cur += c;
            }
        } else if (c == QLatin1Char('"')) {
            quoted = true;
            any = true;
        } else if (c.isSpace()) {
            if (any || !cur.isEmpty())
                args.append(cur);
            cur.clear();
            any = false;
        } else if (c == QLatin1Char('%') && i + 1 < exec.size()) {
            const QChar code = exec.at(++i);
            if (code == QLatin1Char('%'))
                cur += code;
            // %f %F %u %U %i %c %k and the deprecated ones: nothing.
        } else {
            cur += c;
        }
    }
    if (any || !cur.isEmpty())
        args.append(cur);
    return args;
}

QString Apps::findIcon(const QString &icon, const QStringList &dataDirs)
{
    if (icon.isEmpty())
        return {};
    if (QDir::isAbsolutePath(icon))
        return QFile::exists(icon) ? QUrl::fromLocalFile(icon).toString() : QString();
    // The hicolor theme (adaptive icons replace this in Phase 7): the
    // scalable icon, else the largest bitmap.
    static const QStringList sizes = {QStringLiteral("scalable"), QStringLiteral("512x512"), QStringLiteral("256x256"),
                                      QStringLiteral("128x128"), QStringLiteral("96x96"), QStringLiteral("64x64"),
                                      QStringLiteral("48x48")};
    for (const QString &d : dataDirs) {
        for (const QString &s : sizes) {
            for (const char *ext : {".svg", ".png"}) {
                const QString p = d + QStringLiteral("/icons/hicolor/") + s + QStringLiteral("/apps/") + icon + QLatin1String(ext);
                if (QFile::exists(p))
                    return QUrl::fromLocalFile(p).toString();
            }
        }
        for (const char *ext : {".svg", ".png"}) {
            const QString p = d + QStringLiteral("/pixmaps/") + icon + QLatin1String(ext);
            if (QFile::exists(p))
                return QUrl::fromLocalFile(p).toString();
        }
    }
    return {};
}

int Apps::rowCount(const QModelIndex &parent) const
{
    return parent.isValid() ? 0 : int(m_apps.size());
}

QVariant Apps::data(const QModelIndex &index, int role) const
{
    if (!index.isValid() || index.row() >= m_apps.size())
        return {};
    const DesktopEntry &e = m_apps.at(index.row());
    switch (role) {
    case IdRole: return e.id;
    case Qt::DisplayRole:
    case NameRole: return e.name;
    case CommentRole: return e.comment;
    case IconRole: return e.icon;
    default: return {};
    }
}

QHash<int, QByteArray> Apps::roleNames() const
{
    return {{IdRole, "appId"}, {NameRole, "name"}, {CommentRole, "comment"}, {IconRole, "icon"}};
}

QVariantMap Apps::get(const QString &id) const
{
    for (const DesktopEntry &e : m_apps)
        if (e.id == id)
            return {{QStringLiteral("id"), e.id}, {QStringLiteral("name"), e.name},
                    {QStringLiteral("comment"), e.comment}, {QStringLiteral("icon"), e.icon}};
    return {};
}

QString Apps::displayName(const QString &appId) const
{
    const QString name = get(idForWindow(appId)).value(QStringLiteral("name")).toString();
    if (!name.isEmpty())
        return name;
    const QString last = appId.section(QLatin1Char('.'), -1);
    return last.isEmpty() ? appId : last;
}

QString Apps::idForWindow(const QString &appId) const
{
    for (const DesktopEntry &e : m_apps)
        if (e.id == appId)
            return e.id;
    for (const DesktopEntry &e : m_apps)
        if (!e.wmClass.isEmpty() && e.wmClass.compare(appId, Qt::CaseInsensitive) == 0)
            return e.id;
    for (const DesktopEntry &e : m_apps)
        if (e.id.compare(appId, Qt::CaseInsensitive) == 0 || e.id.section(QLatin1Char('.'), -1).compare(appId, Qt::CaseInsensitive) == 0)
            return e.id;
    return appId;
}

int Apps::score(const DesktopEntry &e, const QString &text)
{
    const QString q = text.trimmed();
    if (q.isEmpty())
        return 0;
    const QString name = e.name;
    if (name.startsWith(q, Qt::CaseInsensitive))
        return name.compare(q, Qt::CaseInsensitive) == 0 ? 100 : 90;
    for (const QString &w : name.split(QLatin1Char(' '), Qt::SkipEmptyParts))
        if (w.startsWith(q, Qt::CaseInsensitive))
            return 80;
    if (name.contains(q, Qt::CaseInsensitive))
        return 60;
    if (e.genericName.startsWith(q, Qt::CaseInsensitive))
        return 55;
    for (const QString &k : e.keywords)
        if (k.startsWith(q, Qt::CaseInsensitive))
            return 50;
    if (e.genericName.contains(q, Qt::CaseInsensitive) || e.comment.contains(q, Qt::CaseInsensitive))
        return 30;
    return 0;
}

QVariantList Apps::search(const QString &text, int limit) const
{
    QList<std::pair<int, const DesktopEntry *>> hits;
    for (const DesktopEntry &e : m_apps) {
        if (e.hidden)
            continue;
        if (int s = score(e, text); s > 0)
            hits.append({s, &e});
    }
    std::stable_sort(hits.begin(), hits.end(), [](const auto &a, const auto &b) { return a.first > b.first; });
    QVariantList out;
    for (const auto &[s, e] : hits) {
        if (out.size() >= limit)
            break;
        out.append(QVariantMap{{QStringLiteral("id"), e->id}, {QStringLiteral("name"), e->name},
                               {QStringLiteral("comment"), e->comment.isEmpty() ? e->genericName : e->comment},
                               {QStringLiteral("icon"), e->icon}});
    }
    return out;
}

bool Apps::launch(const QString &id)
{
    for (const DesktopEntry &e : std::as_const(m_apps)) {
        if (e.id != id)
            continue;
        QStringList args = execArgs(e.exec);
        if (args.isEmpty())
            return false;
        const QString program = args.takeFirst();
        QProcess p;
        p.setProgram(program);
        p.setArguments(args);
        p.setWorkingDirectory(QDir::homePath());
        // Apps start with nothing on stdin (ours is the compositor's
        // socket until Qt connects).
        p.setStandardInputFile(QProcess::nullDevice());
        if (!p.startDetached())
            return false;
        emit launched(id);
        return true;
    }
    return false;
}
