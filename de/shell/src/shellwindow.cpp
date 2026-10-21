#include "shellwindow.h"

#include "log.h"

#include "compositor.h"

#include <LayerShellQt/Window>

ShellWindow::ShellWindow(QWindow *parent)
    : QQuickWindow(parent)
{
    setColor(Qt::transparent);
    setFlag(Qt::FramelessWindowHint);
    connect(this, &ShellWindow::configChanged, this, &ShellWindow::applyLayer);
    connect(this, &ShellWindow::blurChanged, this, &ShellWindow::applyBlur);
    connect(this, &ShellWindow::inputRectChanged, this, &ShellWindow::applyInput);
}

void ShellWindow::componentComplete()
{
    m_complete = true;
    applyLayer();
    applyInput();
    if (m_open)
        show();
}

void ShellWindow::setOpen(bool open)
{
    if (open == m_open)
        return;
    m_open = open;
    emit openChanged();
    qCInfo(lcShell) << "window" << m_scope << (open ? "opens" : "closes") << size();
    if (!m_complete)
        return;
    if (open) {
        applyLayer();
        show();
    } else {
        hide();
        // A new surface comes with the next show.
        m_blurSent = false;
    }
}

void ShellWindow::applyLayer()
{
    if (!m_complete)
        return;
    auto *w = LayerShellQt::Window::get(this);
    w->setLayer(LayerShellQt::Window::Layer(m_layer));
    LayerShellQt::Window::Anchors anchors;
    if (m_edges & EdgeTop)
        anchors |= LayerShellQt::Window::AnchorTop;
    if (m_edges & EdgeBottom)
        anchors |= LayerShellQt::Window::AnchorBottom;
    if (m_edges & EdgeLeft)
        anchors |= LayerShellQt::Window::AnchorLeft;
    if (m_edges & EdgeRight)
        anchors |= LayerShellQt::Window::AnchorRight;
    w->setAnchors(anchors);
    w->setExclusiveZone(m_exclusiveZone);
    w->setMargins(QMargins(m_marginLeft, m_marginTop, m_marginRight, m_marginBottom));
    w->setKeyboardInteractivity(m_keyboard == ExclusiveKeyboard ? LayerShellQt::Window::KeyboardInteractivityExclusive
                                : m_keyboard == OnDemandKeyboard ? LayerShellQt::Window::KeyboardInteractivityOnDemand
                                                                 : LayerShellQt::Window::KeyboardInteractivityNone);
    w->setScope(m_scope);
    // Our windows say where they go; never "the screen with the pointer".
    w->setActivateOnShow(m_keyboard != NoKeyboard);
}

void ShellWindow::exposeEvent(QExposeEvent *event)
{
    QQuickWindow::exposeEvent(event);
    qCInfo(lcShell) << "window" << m_scope << (isExposed() ? "exposed" : "not exposed") << size();
    if (isExposed() && !m_blurSent)
        applyBlur();
}

void ShellWindow::applyBlur()
{
    if (!isExposed())
        return;
    Compositor *c = Compositor::instance();
    if (!c)
        return;
    if (m_blurRect.isEmpty())
        c->unsetBlur(this);
    else
        c->setBlur(this, m_blurRect.toAlignedRect(), qRound(m_blurRadius));
    m_blurSent = true;
}

void ShellWindow::applyInput()
{
    if (m_inputRect.isEmpty())
        setMask(QRegion());
    else
        setMask(QRegion(m_inputRect.toAlignedRect()));
}
