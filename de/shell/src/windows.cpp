#include "windows.h"

#include "compositor.h"

#include <QGuiApplication>
#include <QWindow>

using State = QtWayland::zwlr_foreign_toplevel_handle_v1::state;

ToplevelHandle::ToplevelHandle(Windows *model, ::zwlr_foreign_toplevel_handle_v1 *object, int uid)
    : QtWayland::zwlr_foreign_toplevel_handle_v1(object)
    , uid(uid)
    , m_model(model)
{
}

ToplevelHandle::~ToplevelHandle()
{
    if (object())
        destroy();
}

void ToplevelHandle::zwlr_foreign_toplevel_handle_v1_state(wl_array *state)
{
    pending.activated = pending.minimized = pending.maximized = pending.fullscreen = false;
    const auto *s = static_cast<const uint32_t *>(state->data);
    for (size_t i = 0; i < state->size / sizeof(uint32_t); ++i) {
        switch (s[i]) {
        case state_activated: pending.activated = true; break;
        case state_minimized: pending.minimized = true; break;
        case state_maximized: pending.maximized = true; break;
        case state_fullscreen: pending.fullscreen = true; break;
        default: break;
        }
    }
    pending.stateSeen = true;
}

void ToplevelHandle::zwlr_foreign_toplevel_handle_v1_done()
{
    if (!pending.title.isNull())
        title = pending.title;
    if (!pending.appId.isNull())
        appId = pending.appId;
    if (pending.stateSeen) {
        activated = pending.activated;
        minimized = pending.minimized;
        maximized = pending.maximized;
        fullscreen = pending.fullscreen;
    }
    pending = {};
    if (!ready) {
        ready = true;
        m_model->added(this);
    } else {
        m_model->changed(this);
    }
}

void ToplevelHandle::zwlr_foreign_toplevel_handle_v1_closed()
{
    m_model->removed(this);
}

ToplevelManager::ToplevelManager(Windows *model)
    : QWaylandClientExtensionTemplate<ToplevelManager>(3)
    , m_model(model)
{
    initialize();
}

void ToplevelManager::zwlr_foreign_toplevel_manager_v1_toplevel(::zwlr_foreign_toplevel_handle_v1 *toplevel)
{
    // Owned by the model once it is done; until then by the manager.
    new ToplevelHandle(m_model, toplevel, m_model->nextUid());
}

Windows::Windows(QObject *parent)
    : QAbstractListModel(parent)
    , m_manager(new ToplevelManager(this))
{
    m_manager->setParent(this);
}

int Windows::rowCount(const QModelIndex &parent) const
{
    return parent.isValid() ? 0 : int(m_handles.size());
}

QVariant Windows::data(const QModelIndex &index, int role) const
{
    if (!index.isValid() || index.row() >= m_handles.size())
        return {};
    const ToplevelHandle *h = m_handles.at(index.row());
    switch (role) {
    case UidRole: return h->uid;
    case AppIdRole: return h->appId;
    case Qt::DisplayRole:
    case TitleRole: return h->title;
    case ActivatedRole: return h->activated;
    case MinimizedRole: return h->minimized;
    case MaximizedRole: return h->maximized;
    case FullscreenRole: return h->fullscreen;
    default: return {};
    }
}

QHash<int, QByteArray> Windows::roleNames() const
{
    return {{UidRole, "uid"}, {AppIdRole, "appId"}, {TitleRole, "title"}, {ActivatedRole, "activated"},
            {MinimizedRole, "minimized"}, {MaximizedRole, "maximized"}, {FullscreenRole, "fullscreen"}};
}

ToplevelHandle *Windows::find(int uid) const
{
    for (ToplevelHandle *h : m_handles)
        if (h->uid == uid)
            return h;
    return nullptr;
}

void Windows::added(ToplevelHandle *h)
{
    h->setParent(this);
    beginInsertRows({}, int(m_handles.size()), int(m_handles.size()));
    m_handles.append(h);
    endInsertRows();
    emit countChanged();
    changed(h);
}

void Windows::changed(ToplevelHandle *h)
{
    const int row = int(m_handles.indexOf(h));
    if (row < 0)
        return;
    emit dataChanged(index(row), index(row));
    if (h->activated) {
        m_recency.removeAll(h->appId);
        m_recency.prepend(h->appId);
        m_windowRecency.removeAll(h->uid);
        m_windowRecency.prepend(h->uid);
    }
    emit activeChanged();
    emit appIdsChanged();
}

void Windows::removed(ToplevelHandle *h)
{
    const int row = int(m_handles.indexOf(h));
    if (row >= 0) {
        beginRemoveRows({}, row, row);
        m_handles.removeAt(row);
        endRemoveRows();
        emit countChanged();
    }
    m_windowRecency.removeAll(h->uid);
    h->deleteLater();
    emit activeChanged();
    emit appIdsChanged();
}

QStringList Windows::appIds() const
{
    QStringList out;
    for (const ToplevelHandle *h : m_handles)
        if (!out.contains(h->appId))
            out.append(h->appId);
    return out;
}

QString Windows::activeAppId() const
{
    for (const ToplevelHandle *h : m_handles)
        if (h->activated)
            return h->appId;
    return {};
}

QString Windows::activeTitle() const
{
    for (const ToplevelHandle *h : m_handles)
        if (h->activated)
            return h->title;
    return {};
}

QList<int> Windows::windowsOf(const QString &appId) const
{
    QList<int> out;
    for (int uid : m_windowRecency)
        if (const ToplevelHandle *h = find(uid); h && h->appId == appId)
            out.append(uid);
    for (const ToplevelHandle *h : m_handles)
        if (h->appId == appId && !out.contains(h->uid))
            out.append(h->uid);
    return out;
}

void Windows::activate(int uid)
{
    auto *app = qGuiApp->nativeInterface<QNativeInterface::QWaylandApplication>();
    if (ToplevelHandle *h = find(uid); h && app && app->seat())
        h->activate(app->seat());
}

void Windows::minimize(int uid)
{
    if (ToplevelHandle *h = find(uid))
        h->set_minimized();
}

void Windows::close(int uid)
{
    if (ToplevelHandle *h = find(uid))
        h->close();
}

void Windows::setIconRect(int uid, QWindow *window, const QRectF &rect)
{
    ToplevelHandle *h = find(uid);
    wl_surface *s = Compositor::surfaceOf(window);
    if (!h || !s)
        return;
    const QRect r = rect.toAlignedRect();
    h->set_rectangle(s, r.x(), r.y(), r.width(), r.height());
}

void Windows::activateApp(const QString &appId)
{
    const QList<int> uids = windowsOf(appId);
    if (uids.isEmpty())
        return;
    if (activeAppId() == appId) {
        for (int uid : uids)
            minimize(uid);
        return;
    }
    // Back to front, so the most recent ends on top.
    for (auto it = uids.crbegin(); it != uids.crend(); ++it)
        activate(*it);
}

void Windows::closeApp(const QString &appId)
{
    for (int uid : windowsOf(appId))
        close(uid);
}
