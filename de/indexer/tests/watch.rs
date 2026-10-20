//! The crawler and watcher on a real folder: a first crawl, then files
//! made, renamed and deleted, a folder moved in with things inside, and
//! hidden ones left out; then the D-Bus service on a private bus.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fernindex::{Index, watch::Watcher};

fn wait(what: &str, cond: impl Fn() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !cond() {
        assert!(Instant::now() < until, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn has(index: &Mutex<Index>, path: &Path) -> bool {
    index
        .lock()
        .unwrap()
        .contains(path.to_str().unwrap())
        .unwrap()
}

#[test]
fn crawls_then_follows_changes() {
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    std::fs::create_dir_all(h.join("Documents/Old")).unwrap();
    std::fs::write(h.join("Documents/report.txt"), "x").unwrap();
    std::fs::write(h.join("Documents/Old/notes.md"), "x").unwrap();
    std::fs::create_dir_all(h.join(".cache/junk")).unwrap();
    std::fs::write(h.join(".cache/junk/report-cache"), "x").unwrap();
    std::fs::write(h.join(".hidden-report"), "x").unwrap();

    let db = tempfile::NamedTempFile::new().unwrap();
    let index = Arc::new(Mutex::new(Index::open(db.path()).unwrap()));
    // Left from a previous run, gone since: forgotten by the first crawl.
    index
        .lock()
        .unwrap()
        .put(h.join("gone.txt").to_str().unwrap(), false, 1)
        .unwrap();
    let mut w = Watcher::new(h, index.clone()).unwrap();
    w.full_crawl().unwrap();
    assert!(has(&index, &h.join("Documents/Old/notes.md")));
    assert!(!has(&index, &h.join("gone.txt")));
    assert!(!has(&index, &h.join(".hidden-report")));
    assert!(!has(&index, &h.join(".cache/junk/report-cache")));
    let hits = index.lock().unwrap().search("report", 10).unwrap();
    assert_eq!(hits.len(), 1, "{hits:?}");

    std::thread::spawn(move || w.run());

    // Made, renamed, deleted.
    std::fs::write(h.join("Documents/plan.txt"), "x").unwrap();
    wait("a new file", || has(&index, &h.join("Documents/plan.txt")));
    std::fs::rename(
        h.join("Documents/plan.txt"),
        h.join("Documents/final plan.txt"),
    )
    .unwrap();
    wait("a renamed file", || {
        has(&index, &h.join("Documents/final plan.txt"))
            && !has(&index, &h.join("Documents/plan.txt"))
    });
    std::fs::remove_file(h.join("Documents/final plan.txt")).unwrap();
    wait("a deleted file", || {
        !has(&index, &h.join("Documents/final plan.txt"))
    });

    // A folder made elsewhere and moved in, full.
    let outside = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(outside.path().join("Trip/Photos")).unwrap();
    std::fs::write(outside.path().join("Trip/Photos/beach.jpg"), "x").unwrap();
    std::fs::rename(outside.path().join("Trip"), h.join("Trip")).unwrap();
    wait("a folder moved in", || {
        has(&index, &h.join("Trip/Photos/beach.jpg"))
    });
    // And followed there: a change inside it.
    std::fs::write(h.join("Trip/Photos/sunset.jpg"), "x").unwrap();
    wait("a file in a moved-in folder", || {
        has(&index, &h.join("Trip/Photos/sunset.jpg"))
    });

    // A folder renamed: its contents follow, and later changes land under
    // the new name.
    std::fs::rename(h.join("Trip"), h.join("Holiday")).unwrap();
    wait("a renamed folder", || {
        has(&index, &h.join("Holiday/Photos/beach.jpg"))
            && !has(&index, &h.join("Trip/Photos/beach.jpg"))
    });
    std::fs::write(h.join("Holiday/Photos/hill.jpg"), "x").unwrap();
    wait("a file in a renamed folder", || {
        has(&index, &h.join("Holiday/Photos/hill.jpg"))
    });

    // A folder deleted with everything in it.
    std::fs::remove_dir_all(h.join("Documents")).unwrap();
    wait("a deleted folder", || {
        !has(&index, &h.join("Documents/Old/notes.md"))
    });

    // Hidden ones stay out.
    std::fs::write(h.join(".secret"), "x").unwrap();
    std::fs::write(h.join("visible"), "x").unwrap();
    wait("a visible file", || has(&index, &h.join("visible")));
    assert!(!has(&index, &h.join(".secret")));
}

#[test]
fn answers_over_dbus() {
    use std::io::BufRead;
    let tmp = tempfile::tempdir().unwrap();
    let conf = tmp.path().join("bus.conf");
    std::fs::write(
        &conf,
        format!(
            "<busconfig><type>session</type><listen>unix:path={}/bus</listen><policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/><allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>",
            tmp.path().display()
        ),
    )
    .unwrap();
    let Ok(mut daemon) = std::process::Command::new("dbus-daemon")
        .arg(format!("--config-file={}", conf.display()))
        .args(["--nofork", "--print-address"])
        .stdout(std::process::Stdio::piped())
        .spawn()
    else {
        eprintln!("skipped: no dbus-daemon");
        return;
    };
    let mut address = String::new();
    std::io::BufReader::new(daemon.stdout.take().unwrap())
        .read_line(&mut address)
        .unwrap();
    let address = address.trim().to_owned();

    let index = Arc::new(Mutex::new(Index::open(Path::new(":memory:")).unwrap()));
    {
        let i = index.lock().unwrap();
        i.put("/home/sam/Quarterly Report.pdf", false, 1).unwrap();
        i.put("/home/sam/Reports", true, 1).unwrap();
    }
    let ready = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let _server =
        fernindex::bus::serve(Some(&address), fernindex::bus::Search { index, ready }).unwrap();
    let client = zbus::blocking::connection::Builder::address(address.as_str())
        .unwrap()
        .method_timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let reply = client
        .call_method(
            Some(fernindex::bus::NAME),
            fernindex::bus::PATH,
            Some("org.fern.Search1"),
            "Query",
            &("rep", 10u32),
        )
        .unwrap();
    let hits: Vec<(String, String, bool)> = reply.body().deserialize().unwrap();
    assert_eq!(hits.len(), 2);
    // Names starting with the text first.
    assert_eq!(
        hits[0],
        ("/home/sam/Reports".into(), "Reports".into(), true)
    );
    let _ = daemon.kill();
    let _ = daemon.wait();
}
