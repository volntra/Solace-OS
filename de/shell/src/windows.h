// Windows: every app window on the desktop, from wlr-foreign-toplevel
// management (offered to the shell only). The dock shows running apps from
// it, the menu bar the active app's name; it activates, minimizes and
// closes windows, and tells the compositor where each window's dock icon
// is (minimize animates there).
#pragma once

#include <QAbstractListModel>
#include <QPointer>
#include <QtQml/qqmlregistration.h>
#include <QtWaylandClient/QWaylandClientExtension>

#include "qwayland-wlr-foreign-toplevel-management-unstable-v1.h"

class QWindow;
class Windows;

class ToplevelHandle : public QObject, public QtWayland::zwlr_foreign_toplevel_handle_v1
{
    Q_OBJECT
public:
    ToplevelHandle(Windows *model, ::zwlr_foreign_toplevel_handle_v1 *object, int uid);
    ~ToplevelHandle() override;

    int uid;
    QString appId, title;
    bool activated = false, minimized = false, maximized = false, fullscreen = false;
    bool ready = false;

protected:
    void zwlr_foreign_toplevel_handle_v1_title(const QString &title) override { pending.title = title; }
    void zwlr_foreign_toplevel_handle_v1_app_id(const QString &app_id) override { pending.appId = app_id; }
    void zwlr_foreign_toplevel_handle_v1_state(wl_array *state) override;
    void zwlr_foreign_toplevel_handle_v1_done() override;
    void zwlr_foreign_toplevel_handle_v1_closed() override;

private:
    Windows *m_model;
    struct {
        QString appId, title;
        bool activated = false, minimized = false, maximized = false, fullscreen = false;
        bool stateSeen = false;
    } pending;
};

class ToplevelManager : public QWaylandClientExtensionTemplate<ToplevelManager>,
                        public QtWayland::zwlr_foreign_toplevel_manager_v1
{
    Q_OBJECT
public:
    explicit ToplevelManager(Windows *model);

protected:
    void zwlr_foreign_toplevel_manager_v1_toplevel(::zwlr_foreign_toplevel_handle_v1 *toplevel) override;

private:
    Windows *m_model;
};

class Windows : public QAbstractListModel
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    Q_PROPERTY(QString activeAppId READ activeAppId NOTIFY activeChanged)
    Q_PROPERTY(QString activeTitle READ activeTitle NOTIFY activeChanged)
    Q_PROPERTY(int count READ rowCount NOTIFY countChanged)
    // Every window's app id, once each, in the order they appeared.
    Q_PROPERTY(QStringList appIds READ appIds NOTIFY appIdsChanged)

public:
    enum Role { UidRole = Qt::UserRole + 1, AppIdRole, TitleRole, ActivatedRole, MinimizedRole, MaximizedRole, FullscreenRole };

    explicit Windows(QObject *parent = nullptr);

    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

    QString activeAppId() const;
    QString activeTitle() const;
    QStringList appIds() const;

    // The windows of an app (uids), most recently active first.
    Q_INVOKABLE QList<int> windowsOf(const QString &appId) const;
    Q_INVOKABLE bool isRunning(const QString &appId) const { return !windowsOf(appId).isEmpty(); }
    Q_INVOKABLE void activate(int uid);
    Q_INVOKABLE void minimize(int uid);
    Q_INVOKABLE void close(int uid);
    // An app's dock icon is at `rect` on the dock's `window`.
    Q_INVOKABLE void setIconRect(int uid, QWindow *window, const QRectF &rect);
    // Brings an app forward: all its windows, the last active on top; a
    // second click on the frontmost app minimizes it (macOS does the same
    // for a single window).
    Q_INVOKABLE void activateApp(const QString &appId);
    Q_INVOKABLE void closeApp(const QString &appId);

    // For the handles.
    void added(ToplevelHandle *h);
    void changed(ToplevelHandle *h);
    void removed(ToplevelHandle *h);
    int nextUid() { return ++m_lastUid; }

signals:
    void activeChanged();
    void countChanged();
    void appIdsChanged();

private:
    ToplevelHandle *find(int uid) const;

    ToplevelManager *m_manager;
    QList<ToplevelHandle *> m_handles;
    // App ids in order of last activation, newest first.
    QStringList m_recency;
    QList<int> m_windowRecency;
    int m_lastUid = 0;
};
