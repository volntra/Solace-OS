// The tray (StatusNotifierWatcher + host) and DBusMenu against a fake app
// on the session bus: registering, reading properties and icons, clicks,
// changes, the app going away; menus read, bounded, clicked and refreshed.
// Run under its own bus (dbus-run-session).
//
// The fake app answers on its own connection and thread, so the shell
// side's blocking calls get their answers.
#include <QtTest>

#include <QDBusConnection>
#include <QDBusConnectionInterface>
#include <QDBusMessage>
#include <QDBusMetaType>
#include <QDBusObjectPath>
#include <QDBusVirtualObject>
#include <QImage>
#include <QThread>

#include "dbusmenu.h"
#include "tray.h"

struct Node
{
    int id = 0;
    QVariantMap props;
    QList<Node> children;
};
Q_DECLARE_METATYPE(Node)

QDBusArgument &operator<<(QDBusArgument &a, const Node &n)
{
    a.beginStructure();
    a << n.id << n.props;
    a.beginArray(qMetaTypeId<QDBusVariant>());
    for (const Node &c : n.children)
        a << QDBusVariant(QVariant::fromValue(c));
    a.endArray();
    a.endStructure();
    return a;
}
const QDBusArgument &operator>>(const QDBusArgument &a, Node &)
{
    return a;
}

struct Pixmap
{
    int w = 0, h = 0;
    QByteArray argb;
};
Q_DECLARE_METATYPE(Pixmap)
QDBusArgument &operator<<(QDBusArgument &a, const Pixmap &p)
{
    a.beginStructure();
    a << p.w << p.h << p.argb;
    a.endStructure();
    return a;
}
const QDBusArgument &operator>>(const QDBusArgument &a, Pixmap &p)
{
    a.beginStructure();
    a >> p.w >> p.h >> p.argb;
    a.endStructure();
    return a;
}

// The fake app: a status item at /StatusNotifierItem and a menu at /Menu.
class FakeApp : public QDBusVirtualObject
{
    Q_OBJECT
public:
    // Read from the test thread after the app answered; guarded.
    QMutex lock;
    QList<int> clicked;
    QList<QPoint> activated;
    QString title = QStringLiteral("Fake");
    Node menu;

    QString introspect(const QString &) const override { return {}; }

    bool handleMessage(const QDBusMessage &m, const QDBusConnection &conn) override
    {
        QMutexLocker l(&lock);
        const QString member = m.member();
        if (m.path() == QLatin1String("/StatusNotifierItem")) {
            if (member == QLatin1String("GetAll")) {
                // A 2x2 pixmap, opaque red, in network byte order.
                QByteArray argb;
                for (int i = 0; i < 4; ++i)
                    argb.append(QByteArray::fromHex("ffff0000"));
                QVariantMap props{
                    {QStringLiteral("Id"), QStringLiteral("fake")},
                    {QStringLiteral("Title"), title},
                    {QStringLiteral("Status"), QStringLiteral("Active")},
                    {QStringLiteral("Menu"), QVariant::fromValue(QDBusObjectPath(QStringLiteral("/Menu")))},
                    {QStringLiteral("ItemIsMenu"), false},
                    {QStringLiteral("IconPixmap"), QVariant::fromValue(QList<Pixmap>{{2, 2, argb}})},
                };
                conn.send(m.createReply(QVariant::fromValue(props)));
                return true;
            }
            if (member == QLatin1String("Activate")) {
                activated.append({m.arguments().at(0).toInt(), m.arguments().at(1).toInt()});
                conn.send(m.createReply());
                return true;
            }
        }
        if (m.path() == QLatin1String("/Menu")) {
            if (member == QLatin1String("GetLayout")) {
                conn.send(m.createReply(QVariantList{uint(1), QVariant::fromValue(menu)}));
                return true;
            }
            if (member == QLatin1String("Event")) {
                if (m.arguments().at(1).toString() == QLatin1String("clicked"))
                    clicked.append(m.arguments().at(0).toInt());
                conn.send(m.createReply());
                return true;
            }
            if (member == QLatin1String("AboutToShow")) {
                conn.send(m.createReply(false));
                return true;
            }
        }
        return false;
    }
};

namespace {
Node item(int id, const QString &label, QVariantMap extra = {}, QList<Node> children = {})
{
    extra.insert(QStringLiteral("label"), label);
    return {id, extra, children};
}
Node separator(int id)
{
    return {id, {{QStringLiteral("type"), QStringLiteral("separator")}}, {}};
}
}

class TestTray : public QObject
{
    Q_OBJECT
    QThread m_thread;
    FakeApp *m_app = nullptr;
    QString m_appName;

    QDBusConnection appBus() { return QDBusConnection(QStringLiteral("fake-app")); }
    // What the app has seen so far (under its lock).
    template<typename F>
    bool seen(F f)
    {
        QMutexLocker l(&m_app->lock);
        return f();
    }

private slots:
    void initTestCase()
    {
        qDBusRegisterMetaType<Node>();
        qDBusRegisterMetaType<Pixmap>();
        qDBusRegisterMetaType<QList<Pixmap>>();
        qDBusRegisterMetaType<QList<QStringList>>();
        QVERIFY2(QDBusConnection::sessionBus().isConnected(), "run under dbus-run-session");
        QDBusConnection bus = QDBusConnection::connectToBus(QDBusConnection::SessionBus, QStringLiteral("fake-app"));
        QVERIFY(bus.isConnected());
        m_appName = bus.baseService();
        m_thread.start();
        m_app = new FakeApp;
        m_app->moveToThread(&m_thread);
        QVERIFY(bus.registerVirtualObject(QStringLiteral("/StatusNotifierItem"), m_app));
        QVERIFY(bus.registerVirtualObject(QStringLiteral("/Menu"), m_app));
        m_app->menu = item(0, {}, {}, {
            item(1, QStringLiteral("_File"), {}, {
                item(2, QStringLiteral("_Open"), {{QStringLiteral("shortcut"),
                    QVariant::fromValue(QList<QStringList>{{QStringLiteral("Control"), QStringLiteral("O")}})}}),
                separator(3),
                item(4, QStringLiteral("Hidden"), {{QStringLiteral("visible"), false}}),
                item(5, QStringLiteral("Wrap Lines"), {{QStringLiteral("toggle-type"), QStringLiteral("checkmark")},
                                                       {QStringLiteral("toggle-state"), 1}}),
                item(6, QStringLiteral("Quit"), {{QStringLiteral("enabled"), false}}),
            }),
            item(7, QStringLiteral("Save __As")),
        });
    }

    void cleanupTestCase()
    {
        m_thread.quit();
        m_thread.wait();
        delete m_app;
    }

    void addresses()
    {
        const QString me = QStringLiteral(":1.7");
        QCOMPARE(Tray::address(QStringLiteral("/Item"), me), std::make_pair(me, QStringLiteral("/Item")));
        QCOMPARE(Tray::address(QStringLiteral("org.app"), me),
                 std::make_pair(QStringLiteral("org.app"), QStringLiteral("/StatusNotifierItem")));
        QCOMPARE(Tray::address(QStringLiteral(":1.9/Some/Path"), me),
                 std::make_pair(QStringLiteral(":1.9"), QStringLiteral("/Some/Path")));
    }

    void mnemonics()
    {
        QCOMPARE(DBusMenu::stripMnemonic(QStringLiteral("_File")), QStringLiteral("File"));
        QCOMPARE(DBusMenu::stripMnemonic(QStringLiteral("Save __As")), QStringLiteral("Save _As"));
        QCOMPARE(DBusMenu::stripMnemonic(QStringLiteral("Plain")), QStringLiteral("Plain"));
    }

    void menu()
    {
        DBusMenu menu;
        menu.setService(m_appName);
        menu.setPath(QStringLiteral("/Menu"));
        QTRY_COMPARE(menu.items().size(), 2);
        const QVariantMap file = menu.items().at(0).toMap();
        QCOMPARE(file.value("text").toString(), QStringLiteral("File"));
        QCOMPARE(menu.items().at(1).toMap().value("text").toString(), QStringLiteral("Save _As"));
        const QVariantList entries = file.value("children").toList();
        // The hidden one is left out.
        QCOMPARE(entries.size(), 4);
        QCOMPARE(entries.at(0).toMap().value("shortcut").toString(), QStringLiteral("Ctrl O"));
        QVERIFY(entries.at(1).toMap().value("separator").toBool());
        QVERIFY(entries.at(2).toMap().value("checked").toBool());
        QVERIFY(!entries.at(3).toMap().value("enabled").toBool());
        QVERIFY(entries.at(0).toMap().value("enabled").toBool());

        menu.activate(2);
        QTRY_VERIFY(seen([this] { return m_app->clicked.contains(2); }));

        // The app changes its menu and says so.
        {
            QMutexLocker l(&m_app->lock);
            m_app->menu.children.append(item(8, QStringLiteral("Help")));
        }
        QDBusMessage sig = QDBusMessage::createSignal(QStringLiteral("/Menu"), QStringLiteral("com.canonical.dbusmenu"),
                                                      QStringLiteral("LayoutUpdated"));
        sig << uint(2) << 0;
        appBus().send(sig);
        QTRY_COMPARE(menu.items().size(), 3);
        {
            QMutexLocker l(&m_app->lock);
            m_app->menu.children.removeLast();
        }
    }

    void hostileMenusAreBounded()
    {
        // D-Bus itself refuses nesting past 32 containers (three a level).
        Node deep = item(1000, QStringLiteral("leaf"));
        for (int i = 0; i < 9; ++i)
            deep = item(1001 + i, QStringLiteral("level"), {}, {deep});
        QList<Node> wide;
        for (int i = 0; i < 1000; ++i)
            wide.append(item(2000 + i, QStringLiteral("wide")));
        Node saved;
        {
            QMutexLocker l(&m_app->lock);
            saved = m_app->menu;
            m_app->menu = item(0, {}, {}, {deep, item(3, QStringLiteral("Wide"), {}, wide)});
        }
        DBusMenu menu;
        menu.setService(m_appName);
        menu.setPath(QStringLiteral("/Menu"));
        QTRY_COMPARE(menu.items().size(), 2);
        int depth = 0;
        QVariantMap n = menu.items().at(0).toMap();
        while (!n.value("children").toList().isEmpty()) {
            n = n.value("children").toList().at(0).toMap();
            ++depth;
        }
        QVERIFY2(depth <= 6, qPrintable(QString::number(depth)));
        QVERIFY(menu.items().at(1).toMap().value("children").toList().size() <= 200);
        QMutexLocker l(&m_app->lock);
        m_app->menu = saved;
    }

    void tray()
    {
        Tray tray;
        QSignalSpy registered(&tray, &Tray::itemRegistered);
        // The app registers by path: the watcher knows it by its sender.
        QDBusMessage reg = QDBusMessage::createMethodCall(
            QStringLiteral("org.kde.StatusNotifierWatcher"), QStringLiteral("/StatusNotifierWatcher"),
            QStringLiteral("org.kde.StatusNotifierWatcher"), QStringLiteral("RegisterStatusNotifierItem"));
        reg << QStringLiteral("/StatusNotifierItem");
        appBus().asyncCall(reg);
        QTRY_COMPARE(tray.rowCount(), 1);
        QCOMPARE(registered.count(), 1);
        const QModelIndex i = tray.index(0, 0);
        QCOMPARE(tray.data(i, Tray::KeyRole).toString(), m_appName + QStringLiteral("/StatusNotifierItem"));
        QCOMPARE(tray.data(i, Tray::TitleRole).toString(), QStringLiteral("Fake"));
        QCOMPARE(tray.data(i, Tray::MenuRole).toString(), QStringLiteral("/Menu"));
        QCOMPARE(tray.data(i, Tray::ServiceRole).toString(), m_appName);

        // The pixmap arrives as a PNG data URL, red.
        const QString url = tray.data(i, Tray::IconRole).toString();
        QVERIFY(url.startsWith(QStringLiteral("data:image/png;base64,")));
        const QImage img = QImage::fromData(QByteArray::fromBase64(url.mid(22).toLatin1()), "PNG");
        QCOMPARE(img.size(), QSize(2, 2));
        QCOMPARE(img.pixelColor(1, 1), QColor(255, 0, 0));

        // Registered twice: still one.
        appBus().asyncCall(reg);
        QTest::qWait(200);
        QCOMPARE(tray.rowCount(), 1);

        // The watcher lists it.
        QDBusMessage get = QDBusMessage::createMethodCall(
            QStringLiteral("org.kde.StatusNotifierWatcher"), QStringLiteral("/StatusNotifierWatcher"),
            QStringLiteral("org.freedesktop.DBus.Properties"), QStringLiteral("Get"));
        get << QStringLiteral("org.kde.StatusNotifierWatcher") << QStringLiteral("RegisteredStatusNotifierItems");
        QDBusPendingCall pending = appBus().asyncCall(get);
        QTRY_VERIFY(pending.isFinished());
        QCOMPARE(pending.reply().arguments().at(0).value<QDBusVariant>().variant().toStringList(),
                 QStringList{m_appName + QStringLiteral("/StatusNotifierItem")});

        tray.activate(tray.data(i, Tray::KeyRole).toString(), 30, 40);
        QTRY_VERIFY(seen([this] { return m_app->activated.contains(QPoint(30, 40)); }));

        // A new title.
        {
            QMutexLocker l(&m_app->lock);
            m_app->title = QStringLiteral("Renamed");
        }
        appBus().send(QDBusMessage::createSignal(QStringLiteral("/StatusNotifierItem"),
                                                 QStringLiteral("org.kde.StatusNotifierItem"),
                                                 QStringLiteral("NewTitle")));
        QTRY_COMPARE(tray.data(tray.index(0, 0), Tray::TitleRole).toString(), QStringLiteral("Renamed"));

        // The app leaves the bus: its item goes.
        QSignalSpy gone(&tray, &Tray::itemUnregistered);
        QDBusConnection::disconnectFromBus(QStringLiteral("fake-app"));
        QTRY_COMPARE(tray.rowCount(), 0);
        QCOMPARE(gone.count(), 1);
    }
};

QTEST_GUILESS_MAIN(TestTray)
#include "tst_tray.moc"
