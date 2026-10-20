//! fern-settingsd over a real, private dbus-daemon: get, set, validation,
//! change signals, the boot hook and its undo, and the CLI.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use zbus::blocking::{Connection, MessageIterator};
use zbus::zvariant::{OwnedValue, Value};
use zbus::{MatchRule, message};

const NAME: &str = "org.fern.Settings";
const PATH: &str = "/org/fern/Settings";
const IFACE: &str = "org.fern.Settings1";

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A private bus; `None` (and a note) when dbus-daemon is not installed.
fn bus(dir: &Path) -> Option<(Kill, String)> {
    let conf = dir.join("bus.conf");
    fs::write(
        &conf,
        format!(
            r#"<busconfig>
  <type>session</type>
  <listen>unix:path={}/bus</listen>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>"#,
            dir.display()
        ),
    )
    .unwrap();
    let child = match Command::new("dbus-daemon")
        .arg(format!("--config-file={}", conf.display()))
        .args(["--nofork", "--print-address"])
        .stdout(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("SKIPPED: dbus-daemon not available ({e}); run inside `nix develop`");
            return None;
        }
    };
    let mut child = Kill(child);
    let mut line = String::new();
    BufReader::new(child.0.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    Some((child, line.trim().to_owned()))
}

fn daemon(addr: &str, args: &[&str]) -> Kill {
    let child = Command::new(env!("CARGO_BIN_EXE_fern-settingsd"))
        .args(["--address", addr])
        .args(args)
        .spawn()
        .unwrap();
    // Wait for the name.
    let conn = connect(addr);
    let deadline = Instant::now() + Duration::from_secs(10);
    let dbus = zbus::blocking::fdo::DBusProxy::new(&conn).unwrap();
    while !dbus.name_has_owner(NAME.try_into().unwrap()).unwrap() {
        assert!(
            Instant::now() < deadline,
            "fern-settingsd did not take its name"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    Kill(child)
}

fn connect(addr: &str) -> Connection {
    zbus::blocking::connection::Builder::address(addr)
        .unwrap()
        .build()
        .unwrap()
}

fn get(conn: &Connection, d: &str, k: &str) -> OwnedValue {
    conn.call_method(Some(NAME), PATH, Some(IFACE), "Get", &(d, k))
        .unwrap()
        .body()
        .deserialize()
        .unwrap()
}

fn set(conn: &Connection, d: &str, k: &str, v: Value<'_>) -> zbus::Result<message::Message> {
    conn.call_method(Some(NAME), PATH, Some(IFACE), "Set", &(d, k, v))
}

fn error_name(r: zbus::Result<message::Message>) -> String {
    match r {
        Err(zbus::Error::MethodError(name, msg, _)) => {
            format!("{name}: {}", msg.unwrap_or_default())
        }
        other => panic!("expected a D-Bus error, got {other:?}"),
    }
}

#[test]
fn user_settings_over_the_bus() {
    let tmp = tempfile::tempdir().unwrap();
    let Some((_bus, addr)) = bus(tmp.path()) else {
        return;
    };
    let config = tmp.path().join("config");
    let _d = daemon(&addr, &["--dir", config.to_str().unwrap()]);
    let conn = connect(&addr);

    let domains: Vec<String> = conn
        .call_method(Some(NAME), PATH, Some(IFACE), "Domains", &())
        .unwrap()
        .body()
        .deserialize()
        .unwrap();
    assert!(domains.contains(&"appearance".to_owned()) && domains.contains(&"dock".to_owned()));
    assert!(
        !domains.contains(&"boot".to_owned()),
        "boot belongs to the system daemon"
    );

    assert_eq!(
        get(&conn, "appearance", "theme"),
        OwnedValue::from(zbus::zvariant::Str::from("light"))
    );

    // Watch before changing.
    let rule = MatchRule::builder()
        .msg_type(message::Type::Signal)
        .interface(IFACE)
        .unwrap()
        .member("Changed")
        .unwrap()
        .build();
    let mut signals = MessageIterator::for_match_rule(rule, &conn, None).unwrap();

    set(&conn, "appearance", "theme", Value::from("dark")).unwrap();
    // Any integer type is accepted for int keys.
    set(&conn, "dock", "icon_size", Value::from(64u32)).unwrap();
    set(
        &conn,
        "dock",
        "pinned",
        Value::from(vec!["org.example.App"]),
    )
    .unwrap();

    let mut seen = Vec::new();
    for _ in 0..3 {
        let m = signals.next().unwrap().unwrap();
        let (d, k, v): (String, String, OwnedValue) = m.body().deserialize().unwrap();
        seen.push(format!(
            "{d}.{k}={}",
            settingsd::Value::from_variant(&v).unwrap()
        ));
    }
    assert_eq!(
        seen,
        [
            "appearance.theme=dark",
            "dock.icon_size=64",
            "dock.pinned=org.example.App"
        ]
    );
    assert_eq!(get(&conn, "dock", "icon_size"), OwnedValue::from(64i64));

    let text = fs::read_to_string(config.join("appearance.toml")).unwrap();
    assert!(text.contains("theme = \"dark\""), "{text}");

    // Refused: out of range, wrong type, unknown names.
    assert!(error_name(set(&conn, "dock", "icon_size", Value::from(5i64))).contains("InvalidArgs"));
    assert!(
        error_name(set(&conn, "dock", "icon_size", Value::from(1.5f64))).contains("InvalidArgs")
    );
    assert!(error_name(set(&conn, "dock", "colour", Value::from(true))).contains("InvalidArgs"));
    assert!(error_name(set(&conn, "nope", "x", Value::from(true))).contains("InvalidArgs"));

    conn.call_method(
        Some(NAME),
        PATH,
        Some(IFACE),
        "Reset",
        &("appearance", "theme"),
    )
    .unwrap();
    assert_eq!(
        get(&conn, "appearance", "theme"),
        OwnedValue::from(zbus::zvariant::Str::from("light"))
    );

    // The command-line tool talks to the same daemon.
    let cli = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_fern-settings"))
            .args(["--address", &addr])
            .args(args)
            .output()
            .unwrap()
    };
    let out = cli(&["set", "dock", "position", "left"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = cli(&["get", "dock", "position"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "left\n");
    let out = cli(&["set", "dock", "magnification", "maybe"]);
    assert!(!out.status.success());
    let out = cli(&["set", "dock", "position", "top"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("not one of bottom, left, right"));
    let out = cli(&["list", "dock"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("pinned = org.example.App"));
}

#[test]
fn system_boot_settings_run_the_hook_and_undo_on_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let Some((_bus, addr)) = bus(tmp.path()) else {
        return;
    };
    let etc = tmp.path().join("etc");
    fs::create_dir_all(&etc).unwrap();
    fs::write(etc.join("boot.toml"), "menu = \"always\"\n").unwrap();
    // The hook stands in for `spk boot-menu`: it records the file it saw,
    // and fails while a "full" marker exists.
    let hook = tmp.path().join("hook.sh");
    let log = tmp.path().join("hook.log");
    let full = tmp.path().join("full");
    fs::write(
        &hook,
        format!(
            "#!/bin/sh\ncat {etc}/boot.toml >> {log}\nif [ -e {full} ]; then echo 'not enough space on the ESP' >&2; exit 1; fi\n",
            etc = etc.display(),
            log = log.display(),
            full = full.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&hook, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();

    let _d = daemon(
        &addr,
        &[
            "--system",
            "--dir",
            etc.to_str().unwrap(),
            "--boot-hook",
            hook.to_str().unwrap(),
        ],
    );
    let conn = connect(&addr);
    assert_eq!(
        get(&conn, "boot", "menu"),
        OwnedValue::from(zbus::zvariant::Str::from("always"))
    );

    set(&conn, "boot", "timeout", Value::from(9i64)).unwrap();
    assert!(fs::read_to_string(&log).unwrap().contains("timeout = 9"));
    let b: bootgen::Settings =
        toml::from_str(&fs::read_to_string(etc.join("boot.toml")).unwrap()).unwrap();
    assert_eq!((b.menu, b.timeout), (bootgen::MenuStyle::Always, 9));

    // The hook fails: the change is undone and the error says why.
    fs::write(&full, "").unwrap();
    let e = error_name(set(&conn, "boot", "keep", Value::from(7i64)));
    assert!(
        e.contains("Failed") && e.contains("not enough space on the ESP"),
        "{e}"
    );
    let b: bootgen::Settings =
        toml::from_str(&fs::read_to_string(etc.join("boot.toml")).unwrap()).unwrap();
    assert_eq!(b.keep, 3);
    assert_eq!(get(&conn, "boot", "keep"), OwnedValue::from(3i64));
    fs::remove_file(&full).unwrap();

    // Another program (spk rollback) edits the file: the next read sees it.
    fs::write(
        etc.join("boot.toml"),
        "menu = \"always\"\ntimeout = 9\ndefault = \"4\"\n",
    )
    .unwrap();
    assert_eq!(
        get(&conn, "boot", "default"),
        OwnedValue::from(zbus::zvariant::Str::from("4"))
    );

    // User domains are not served by the system daemon.
    assert!(error_name(set(&conn, "dock", "autohide", Value::from(true))).contains("InvalidArgs"));
}
