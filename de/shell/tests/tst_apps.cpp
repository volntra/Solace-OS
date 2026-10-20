// Desktop entries: parsing, localisation, Exec quoting, search order.
#include <QtTest>

#include "apps.h"

class TestApps : public QObject
{
    Q_OBJECT
private slots:
    void parse()
    {
        const QByteArray data = R"([Desktop Entry]
Type=Application
Name=Terminal
Name[de]=Terminal (de)
Name[de_AT]=Terminal (at)
GenericName=Terminal Emulator
Comment=Use the command line
Exec=fern-terminal --new-window %U
Icon=org.fern.Terminal
Keywords=shell;prompt;command;
Categories=System;TerminalEmulator;
StartupWMClass=fern-terminal

[Desktop Action NewWindow]
Name=New Window
Exec=should-not-win
)";
        DesktopEntry e = Apps::parse(QStringLiteral("org.fern.Terminal"), data, QStringLiteral("de_AT"));
        QCOMPARE(e.name, QStringLiteral("Terminal (at)"));
        QCOMPARE(e.exec, QStringLiteral("fern-terminal --new-window %U"));
        QCOMPARE(e.keywords, (QStringList{"shell", "prompt", "command"}));
        QCOMPARE(e.wmClass, QStringLiteral("fern-terminal"));
        QVERIFY(!e.hidden);
        QCOMPARE(Apps::parse(QStringLiteral("x"), data, QStringLiteral("de_DE")).name, QStringLiteral("Terminal (de)"));
        QCOMPARE(Apps::parse(QStringLiteral("x"), data, QStringLiteral("fr_FR")).name, QStringLiteral("Terminal"));

        // Not an application: nothing to launch.
        QVERIFY(Apps::parse(QStringLiteral("l"), "[Desktop Entry]\nType=Link\nName=L\nURL=x\n", {}).exec.isEmpty());
        QVERIFY(Apps::parse(QStringLiteral("h"), "[Desktop Entry]\nType=Application\nName=H\nExec=h\nNoDisplay=true\n", {}).hidden);
    }

    void exec_args()
    {
        QCOMPARE(Apps::execArgs(QStringLiteral("app %U")), QStringList{"app"});
        QCOMPARE(Apps::execArgs(QStringLiteral("\"/opt/My App/bin/app\" --flag %f")),
                 (QStringList{"/opt/My App/bin/app", "--flag"}));
        QCOMPARE(Apps::execArgs(QStringLiteral("sh -c \"echo \\\"hi\\\"\"")),
                 (QStringList{"sh", "-c", "echo \"hi\""}));
        QCOMPARE(Apps::execArgs(QStringLiteral("app 100%%")), (QStringList{"app", "100%"}));
        QCOMPARE(Apps::execArgs(QStringLiteral("app \"\"")), (QStringList{"app", ""}));
    }

    void search_order()
    {
        QTemporaryDir dir;
        auto write = [&](const char *id, const char *body) {
            QFile f(dir.filePath(QString::fromLatin1(id) + QStringLiteral(".desktop")));
            QVERIFY(f.open(QIODevice::WriteOnly));
            f.write(QByteArray("[Desktop Entry]\nType=Application\nExec=true\n") + body);
        };
        write("org.fern.Terminal", "Name=Terminal\nKeywords=shell;console;\n");
        write("org.fern.Files", "Name=Files\nComment=Browse your terminal output\n");
        write("org.example.TextEditor", "Name=Text Editor\n");
        write("org.example.Hidden", "Name=Termination\nNoDisplay=true\n");
        Apps apps;
        apps.loadFrom({dir.path()});
        QCOMPARE(apps.rowCount(), 4);
        const QVariantList hits = apps.search(QStringLiteral("term"));
        QCOMPARE(hits.size(), 2);
        QCOMPARE(hits.at(0).toMap().value("id").toString(), QStringLiteral("org.fern.Terminal"));
        QCOMPARE(hits.at(1).toMap().value("id").toString(), QStringLiteral("org.fern.Files"));
        QCOMPARE(apps.search(QStringLiteral("edit")).at(0).toMap().value("id").toString(),
                 QStringLiteral("org.example.TextEditor"));
        QCOMPARE(apps.search(QStringLiteral("console")).size(), 1);
        // Window app ids: exact, case-insensitive, last part of the id.
        QCOMPARE(apps.idForWindow(QStringLiteral("org.fern.Files")), QStringLiteral("org.fern.Files"));
        QCOMPARE(apps.idForWindow(QStringLiteral("terminal")), QStringLiteral("org.fern.Terminal"));
        QCOMPARE(apps.idForWindow(QStringLiteral("unknown")), QStringLiteral("unknown"));
        // Names: the entry's, else the id's last part.
        QCOMPARE(apps.displayName(QStringLiteral("terminal")), QStringLiteral("Terminal"));
        QCOMPARE(apps.displayName(QStringLiteral("org.example.Editor")), QStringLiteral("Editor"));
        QCOMPARE(apps.displayName(QStringLiteral("plain")), QStringLiteral("plain"));
    }
};

QTEST_GUILESS_MAIN(TestApps)
#include "tst_apps.moc"
