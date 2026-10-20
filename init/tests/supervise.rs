//! Runs the real fern-init binary against a scratch root and drives it with
//! the real fernctl binary.

use std::fs;
use std::path::Path;
use std::process::{Child, Command, Output};
use std::time::{Duration, Instant};

const INIT: &str = env!("CARGO_BIN_EXE_fern-init");
const CTL: &str = env!("CARGO_BIN_EXE_fernctl");

struct Init(Child);

impl Drop for Init {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn unit(root: &Path, name: &str, body: &str) {
    let d = root.join("usr/lib/fern/units");
    fs::create_dir_all(&d).unwrap();
    // Units get a clean environment; give the test's PATH to the shell.
    let path = std::env::var("PATH").unwrap_or_default();
    let body = body.replace(
        "[service]",
        &format!("[service]\nenvironment = {{ PATH = \"{path}\" }}"),
    );
    fs::write(d.join(format!("{name}.toml")), body).unwrap();
}

fn ctl(root: &Path, args: &[&str]) -> Output {
    Command::new(CTL)
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn wait_for(what: &str, mut f: impl FnMut() -> bool) {
    let t = Instant::now();
    while !f() {
        assert!(
            t.elapsed() < Duration::from_secs(15),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The STATE column of `fernctl status <unit>` (the second line's second
/// field), so a description such as "long running" cannot match.
fn state(root: &Path, unit: &str) -> String {
    stdout(&ctl(root, &["status", unit]))
        .lines()
        .nth(1)
        .and_then(|l| l.split_whitespace().nth(1))
        .unwrap_or_default()
        .to_string()
}

fn pid_alive(pid: u32) -> bool {
    // A zombie is not alive for our purposes.
    fs::read_to_string(format!("/proc/{pid}/stat"))
        .is_ok_and(|s| s.split_whitespace().nth(2).is_none_or(|st| st != "Z"))
}

#[test]
// One scenario, read top to bottom: boot, supervise, control, shut down.
#[allow(clippy::too_many_lines)]
fn supervises_a_scratch_root() {
    if !Path::new("/bin/sh").exists() {
        eprintln!("skipping: no /bin/sh");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    unit(
        root,
        "base",
        "description = \"prepares things\"\nenabled = true\n[service]\nexec = [\"/bin/sh\", \"-c\", \"echo base-ran\"]\ntype = \"oneshot\"\nrestart = \"no\"\n",
    );
    unit(
        root,
        "app",
        "description = \"long running\"\nenabled = true\nrequires = [\"base\"]\n[service]\nexec = [\"/bin/sh\", \"-c\", \"echo app-up; echo oops >&2; exec sleep 300\"]\n",
    );
    unit(
        root,
        "crashy",
        "description = \"crashes\"\nenabled = true\n[service]\nexec = [\"/bin/sh\", \"-c\", \"echo crash; exit 3\"]\nrestart = \"always\"\nrestart_delay_ms = 50\n",
    );
    // Each service is its own session (agetty needs that to take its tty
    // as controlling terminal). Field 6 of /proc/<pid>/stat is the session.
    unit(
        root,
        "session",
        "description = \"session check\"\nenabled = true\n[service]\nexec = [\"/bin/sh\", \"-c\", \"set -- $(cat /proc/$$/stat); if [ \\\"$6\\\" = \\\"$$\\\" ]; then echo own-session; else echo shared-session; fi\"]\ntype = \"oneshot\"\nrestart = \"no\"\n",
    );
    unit(
        root,
        "manual",
        "description = \"not enabled\"\n[service]\nexec = [\"/bin/sh\", \"-c\", \"exec sleep 300\"]\n",
    );

    let init = Init(
        Command::new(INIT)
            .arg("--root")
            .arg(root)
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_for("control socket", || root.join("run/fern/control").exists());

    // Ordering: app starts after base finished.
    wait_for("app running", || state(root, "app") == "running");
    let st = stdout(&ctl(root, &["status"]));
    assert!(st.contains("base") && st.contains("done"), "{st}");
    assert!(st.contains("manual") && st.contains("inactive"), "{st}");
    let logs = stdout(&ctl(root, &["logs", "-n", "500"]));
    let base_at = logs.find("base-ran").expect("base output logged");
    let app_at = logs.find("app-up").expect("app output logged");
    assert!(base_at < app_at, "{logs}");
    assert!(logs.contains("oops"), "stderr captured: {logs}");
    wait_for("session check", || {
        stdout(&ctl(root, &["logs", "session"])).contains("-session")
    });
    let session = stdout(&ctl(root, &["logs", "session"]));
    assert!(session.contains("own-session"), "{session}");

    // Crash loop: restarted several times, with backoff.
    wait_for("crashy restarts", || {
        stdout(&ctl(root, &["logs", "crashy", "-n", "500"]))
            .lines()
            .filter(|l| l.ends_with(" crash"))
            .count()
            >= 3
    });

    // Boot completed and was measured.
    wait_for("boot time", || {
        stdout(&ctl(root, &["boot-time"])).contains("ms in fern-init")
    });
    assert!(root.join("run/fern/boot-time").exists());

    // Manual start and stop.
    assert!(ctl(root, &["start", "manual"]).status.success());
    wait_for("manual running", || state(root, "manual") == "running");
    let pid: u32 = stdout(&ctl(root, &["status", "manual"]))
        .lines()
        .nth(1)
        .and_then(|l| l.split_whitespace().nth(2))
        .and_then(|p| p.parse().ok())
        .expect("pid in status");
    assert!(pid_alive(pid));
    assert!(ctl(root, &["stop", "manual"]).status.success());
    wait_for("manual stopped", || {
        stdout(&ctl(root, &["status", "manual"])).contains("stopped")
    });
    wait_for("manual's process gone", || !pid_alive(pid));

    // Enable / disable persist as files.
    assert!(ctl(root, &["enable", "manual"]).status.success());
    assert!(root.join("etc/fern/enabled/manual").exists());
    assert!(ctl(root, &["disable", "manual"]).status.success());
    assert!(root.join("etc/fern/disabled/manual").exists());
    assert!(!root.join("etc/fern/enabled/manual").exists());

    // Errors are reported, not ignored.
    let bad = ctl(root, &["start", "nope"]);
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("no unit named nope"));

    // Clean shutdown: every service process is gone and init exits 0.
    let app_pid: u32 = stdout(&ctl(root, &["status", "app"]))
        .lines()
        .nth(1)
        .and_then(|l| l.split_whitespace().nth(2))
        .and_then(|p| p.parse().ok())
        .expect("app pid");
    assert!(ctl(root, &["poweroff"]).status.success());
    let mut init = init;
    let t = Instant::now();
    let status = loop {
        if let Some(s) = init.0.try_wait().unwrap() {
            break s;
        }
        assert!(
            t.elapsed() < Duration::from_secs(15),
            "fern-init did not exit"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(status.success());
    assert!(!pid_alive(app_pid), "app's process survived shutdown");

    // The persistent log has the whole story.
    let log = fs::read_to_string(root.join("var/log/fern/log.jsonl")).unwrap();
    assert!(log.contains("\"base-ran\"") && log.contains("shutting down"));
}

#[test]
fn refuses_dependency_cycles_at_startup() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    unit(
        root,
        "a",
        "description = \"a\"\nrequires = [\"b\"]\n[service]\nexec = [\"/bin/true\"]\n",
    );
    unit(
        root,
        "b",
        "description = \"b\"\nrequires = [\"a\"]\n[service]\nexec = [\"/bin/true\"]\n",
    );
    let out = Command::new(INIT).arg("--root").arg(root).output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("cycle"));
}
