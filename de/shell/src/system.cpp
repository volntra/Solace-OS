#include "system.h"

#include <QDir>
#include <QFile>
#include <QProcess>
#include <QRegularExpression>
#include <QStandardPaths>
#include <QSysInfo>
#include <cmath>

namespace {
QString readFile(const QString &path)
{
    QFile f(path);
    return f.open(QIODevice::ReadOnly) ? QString::fromUtf8(f.readAll()) : QString();
}
}

System::System(QObject *parent)
    : QObject(parent)
{
    QString rel = readFile(QStringLiteral("/etc/os-release"));
    if (rel.isEmpty())
        rel = readFile(QStringLiteral("/usr/lib/os-release"));
    const QString name = osReleaseValue(rel, QStringLiteral("NAME"));
    // Only on Fern itself: a development run on another system says so.
    m_version = name == QLatin1String("Fern") ? osReleaseValue(rel, QStringLiteral("VERSION"))
                                              : QStringLiteral("development build");
    m_processor = cpuModel(readFile(QStringLiteral("/proc/cpuinfo")));
    m_memory = memoryTotal(readFile(QStringLiteral("/proc/meminfo")));
    // The first display device's driver, e.g. "nvidia", "virtio-pci".
    const QDir drm(QStringLiteral("/sys/class/drm"));
    for (const QString &card : drm.entryList({QStringLiteral("card?")}, QDir::Dirs | QDir::System)) {
        const QString uevent = readFile(drm.filePath(card + QStringLiteral("/device/uevent")));
        const auto m = QRegularExpression(QStringLiteral("DRIVER=(\\S+)")).match(uevent);
        if (m.hasMatch()) {
            const QString d = m.captured(1);
            m_graphics = d == QLatin1String("nvidia") ? QStringLiteral("NVIDIA")
                         : d == QLatin1String("amdgpu") ? QStringLiteral("AMD Radeon")
                         : d == QLatin1String("i915") || d == QLatin1String("xe") ? QStringLiteral("Intel Graphics")
                         : d.startsWith(QLatin1String("virtio")) ? QStringLiteral("Virtual display")
                                                                 : d;
            break;
        }
    }
}

QString System::osReleaseValue(const QString &text, const QString &key)
{
    for (const QString &line : text.split(QLatin1Char('\n'))) {
        if (!line.startsWith(key + QLatin1Char('=')))
            continue;
        QString v = line.mid(key.size() + 1).trimmed();
        if (v.size() >= 2 && (v.startsWith(QLatin1Char('"')) || v.startsWith(QLatin1Char('\''))))
            v = v.mid(1, v.size() - 2);
        return v;
    }
    return {};
}

QString System::cpuModel(const QString &cpuinfo)
{
    int cores = 0;
    QString model;
    for (const QString &line : cpuinfo.split(QLatin1Char('\n'))) {
        if (line.startsWith(QLatin1String("model name"))) {
            ++cores;
            if (model.isEmpty())
                model = line.section(QLatin1Char(':'), 1).trimmed();
        }
    }
    // "AMD Ryzen 7 7800X3D 8-Core Processor" already says how many cores.
    model.replace(QRegularExpression(QStringLiteral("\\s+")), QStringLiteral(" "));
    if (model.isEmpty())
        return {};
    return cores > 1 && !model.contains(QLatin1String("Core")) ? QStringLiteral("%1 (%2 threads)").arg(model).arg(cores) : model;
}

QString System::memoryTotal(const QString &meminfo)
{
    const auto m = QRegularExpression(QStringLiteral("MemTotal:\\s+(\\d+) kB")).match(meminfo);
    if (!m.hasMatch())
        return {};
    // Rounded to what is printed on the box (installed, not usable).
    const double gib = m.captured(1).toDouble() / 1024 / 1024;
    const double rounded = gib > 6 ? std::ceil(gib / 2) * 2 : std::ceil(gib);
    return QStringLiteral("%1 GB").arg(rounded);
}

QString System::home() const
{
    return QDir::homePath();
}

void System::emptyTrash()
{
    const QString base = qEnvironmentVariable("XDG_DATA_HOME", QDir::homePath() + QStringLiteral("/.local/share"))
                         + QStringLiteral("/Trash");
    for (const char *sub : {"files", "info"}) {
        QDir d(base + QLatin1Char('/') + QLatin1String(sub));
        for (const QFileInfo &fi : d.entryInfoList(QDir::AllEntries | QDir::NoDotAndDotDot | QDir::Hidden | QDir::System)) {
            if (fi.isDir() && !fi.isSymLink())
                QDir(fi.filePath()).removeRecursively();
            else
                QFile::remove(fi.filePath());
        }
    }
}

QVariantList System::listFolder(const QString &path, int limit) const
{
    QVariantList out;
    const QFileInfoList entries = QDir(path).entryInfoList(QDir::AllEntries | QDir::NoDotAndDotDot, QDir::Time);
    for (const QFileInfo &fi : entries) {
        if (out.size() >= limit)
            break;
        out.append(QVariantMap{{QStringLiteral("name"), fi.fileName()},
                               {QStringLiteral("size"), fi.isDir() ? 0 : double(fi.size())},
                               {QStringLiteral("modified"), fi.lastModified()},
                               {QStringLiteral("dir"), fi.isDir()}});
    }
    return out;
}

QString System::hostName() const
{
    return QSysInfo::machineHostName();
}

bool System::canPower() const
{
    return !QStandardPaths::findExecutable(QStringLiteral("fernctl")).isEmpty();
}

void System::restart()
{
    QProcess::startDetached(QStringLiteral("fernctl"), {QStringLiteral("reboot")});
}

void System::shutDown()
{
    QProcess::startDetached(QStringLiteral("fernctl"), {QStringLiteral("poweroff")});
}
