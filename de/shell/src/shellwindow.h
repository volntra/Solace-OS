// ShellWindow: a Qt Quick window that is a wlr-layer-shell surface (a
// part of the shell, not an app window): which layer, which screen edges
// it sticks to, how much room it keeps for itself, whether it takes the
// keyboard, and the rounded blur behind it (fern_shell_v1). Show and hide
// it with `open`, never `visible`: the layer must be set before the
// surface exists.
#pragma once

#include <QQmlParserStatus>
#include <QQuickWindow>
#include <QtQml/qqmlregistration.h>

class ShellWindow : public QQuickWindow, public QQmlParserStatus
{
    Q_OBJECT
    Q_INTERFACES(QQmlParserStatus)
    QML_ELEMENT
    Q_PROPERTY(Layer layer MEMBER m_layer NOTIFY configChanged)
    Q_PROPERTY(int edges MEMBER m_edges NOTIFY configChanged)
    Q_PROPERTY(int exclusiveZone MEMBER m_exclusiveZone NOTIFY configChanged)
    Q_PROPERTY(Keyboard keyboard MEMBER m_keyboard NOTIFY configChanged)
    Q_PROPERTY(int marginTop MEMBER m_marginTop NOTIFY configChanged)
    Q_PROPERTY(int marginBottom MEMBER m_marginBottom NOTIFY configChanged)
    Q_PROPERTY(int marginLeft MEMBER m_marginLeft NOTIFY configChanged)
    Q_PROPERTY(int marginRight MEMBER m_marginRight NOTIFY configChanged)
    Q_PROPERTY(QString scope MEMBER m_scope NOTIFY configChanged)
    Q_PROPERTY(bool open READ isOpen WRITE setOpen NOTIFY openChanged)
    // Blur behind this rectangle (window coordinates), rounded; an empty
    // rectangle means none.
    Q_PROPERTY(QRectF blurRect MEMBER m_blurRect NOTIFY blurChanged)
    Q_PROPERTY(qreal blurRadius MEMBER m_blurRadius NOTIFY blurChanged)
    // Where the window takes pointer input; empty: everywhere.
    Q_PROPERTY(QRectF inputRect MEMBER m_inputRect NOTIFY inputRectChanged)

public:
    enum Layer { Background, Bottom, Top, Overlay };
    Q_ENUM(Layer)
    enum Edge { EdgeTop = 1, EdgeBottom = 2, EdgeLeft = 4, EdgeRight = 8 };
    Q_ENUM(Edge)
    enum Keyboard { NoKeyboard, ExclusiveKeyboard, OnDemandKeyboard };
    Q_ENUM(Keyboard)

    explicit ShellWindow(QWindow *parent = nullptr);

    bool isOpen() const { return m_open; }
    void setOpen(bool open);

    void classBegin() override {}
    void componentComplete() override;

signals:
    void configChanged();
    void openChanged();
    void blurChanged();
    void inputRectChanged();

protected:
    void exposeEvent(QExposeEvent *event) override;

private:
    void applyLayer();
    void applyBlur();
    void applyInput();

    Layer m_layer = Top;
    int m_edges = 0;
    int m_exclusiveZone = 0;
    Keyboard m_keyboard = NoKeyboard;
    int m_marginTop = 0, m_marginBottom = 0, m_marginLeft = 0, m_marginRight = 0;
    QString m_scope = QStringLiteral("fern-shell");
    bool m_open = false;
    bool m_complete = false;
    QRectF m_blurRect;
    qreal m_blurRadius = 0;
    QRectF m_inputRect;
    bool m_blurSent = false;
};
