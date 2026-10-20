// Screenshots (CLAUDE.md 6.8): the compositor takes the picture
// (Compositor.screenshot, without the pointer); this saves it as PNG in
// ~/Pictures/Screenshots, named by date and time as
// "Screenshot 2026-10-20 at 21.04.07.png". Screen recording is not here
// yet (it needs PipeWire; see docs/STATUS.md).
#pragma once

#include <QDateTime>
#include <QImage>
#include <QObject>
#include <QtQml/qqmlregistration.h>

class Screenshots : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON

public:
    using QObject::QObject;

    // Saves a picture the compositor took (Compositor.screenshotTaken)
    // under today's name; returns its path, empty on failure.
    Q_INVOKABLE QString saveNow(const QImage &image) const;

    // Where they go, and what they are called; public for tests.
    Q_INVOKABLE static QString folder();
    static QString fileName(const QDateTime &when);
    // `name` in `dir`, or "name 2.png", "name 3.png"... if taken.
    static QString freePath(const QString &dir, const QString &name);
    // Saves `image` in `dir` and returns its path, or empty on failure.
    static QString save(const QImage &image, const QString &dir, const QDateTime &when);
};
