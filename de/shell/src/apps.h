// Apps: every app the user can open, from freedesktop desktop entries
// (XDG_DATA_HOME and XDG_DATA_DIRS /applications). App bundles in
// /Applications join in Phase 11. Search and the dock read it; launch()
// starts an app.
#pragma once

#include <QAbstractListModel>
#include <QFileSystemWatcher>
#include <QtQml/qqmlregistration.h>

struct DesktopEntry
{
    QString id;       // file name without .desktop: org.fern.Files
    QString name, genericName, comment, exec, icon, wmClass;
    QStringList keywords, categories, mimeTypes;
    bool terminal = false;
    bool hidden = false; // NoDisplay or Hidden: not listed, still launchable
};

class Apps : public QAbstractListModel
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    Q_PROPERTY(int count READ rowCount NOTIFY countChanged)

public:
    enum Role { IdRole = Qt::UserRole + 1, NameRole, CommentRole, IconRole };

    explicit Apps(QObject *parent = nullptr);

    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

    // id, name, comment, icon (a file URL or empty); empty map if unknown.
    Q_INVOKABLE QVariantMap get(const QString &id) const;
    // The desktop entry for a window's app id (exact, StartupWMClass, or
    // case-insensitive), or the app id itself when none matches.
    Q_INVOKABLE QString idForWindow(const QString &appId) const;
    // What to call a window's app: its entry's name, else the last part
    // of a reverse-DNS id ("org.example.Editor" -> "Editor").
    Q_INVOKABLE QString displayName(const QString &appId) const;
    // Apps matching `text`, best first: name prefix, word prefix, name,
    // keywords, comment.
    Q_INVOKABLE QVariantList search(const QString &text, int limit = 8) const;
    Q_INVOKABLE bool launch(const QString &id);
    // Opens a file or folder in its app; false when no app can.
    Q_INVOKABLE bool open(const QString &path);

    // Parsing, public for tests.
    static DesktopEntry parse(const QString &id, const QByteArray &data, const QString &locale);
    static QStringList execArgs(const QString &exec, const QStringList &files = {});
    // The app for a file of MIME type `mime` (with its parent types, most
    // specific first): the user's default from mimeapps.list files (most
    // important first), else the first entry handling it. Empty if none.
    static QString chooseApp(const QStringList &mimeChain, const QList<DesktopEntry> &apps,
                             const QList<QByteArray> &mimeappsLists);
    QString appFor(const QString &path) const;
    static int score(const DesktopEntry &e, const QString &text);
    void loadFrom(const QStringList &dirs);
    static QString findIcon(const QString &icon, const QStringList &dataDirs);

signals:
    void countChanged();
    void launched(const QString &id);

private:
    void rescan();
    QStringList appDirs() const;

    QList<DesktopEntry> m_apps;
    QFileSystemWatcher m_watcher;
    QStringList m_dataDirs;
};
