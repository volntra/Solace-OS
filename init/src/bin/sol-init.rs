//! sol-init: PID 1, or (with `--root DIR`) a supervisor for a scratch root.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use solinit::log::{LOG_FILE, LogStore};
use solinit::pid1::{self, Cmdline};
use solinit::runtime::{Mode, Runtime};

fn main() -> ExitCode {
    if rustix::process::getpid() == rustix::process::Pid::INIT {
        pid1_main()
    }
    let mut args = std::env::args().skip(1);
    let mut root = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--root" => root = args.next().map(PathBuf::from),
            "-h" | "--help" => {
                println!("usage: sol-init --root <dir>   (as PID 1 it takes no arguments)");
                return ExitCode::SUCCESS;
            }
            _ => {
                eprintln!("sol-init: unknown argument {a}");
                return ExitCode::from(2);
            }
        }
    }
    let Some(root) = root else {
        eprintln!("sol-init: not PID 1; pass --root <dir> to supervise a scratch root");
        return ExitCode::from(2);
    };
    let mut log = LogStore::memory_only();
    log.echo = true;
    if let Err(e) = log.persist_to(&root.join(LOG_FILE)) {
        eprintln!("sol-init: log: {e}");
    }
    match Runtime::new(&root, Mode::User, log) {
        Ok(mut rt) => {
            let p = rt.run();
            rt.log.push("sol-init", "init", &format!("exiting ({p:?})"));
            rt.log.flush();
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("sol-init: {e}");
            ExitCode::FAILURE
        }
    }
}

/// As PID 1: in the initramfs, assemble the real root and hand over to its
/// sol-init; in the real root, set up the system and supervise it.
fn pid1_main() -> ! {
    pid1::early_mounts();
    let cmdline = Cmdline::parse(&std::fs::read_to_string("/proc/cmdline").unwrap_or_default());
    // The initramfs has no /usr: our job there is to find and mount it.
    if !Path::new(pid1::INIT_PATH).exists() {
        initramfs(&cmdline);
    }
    let mut log = LogStore::memory_only();
    log.echo = cmdline.verbose;
    // Ctrl-Alt-Del sends us SIGINT instead of rebooting immediately.
    let _ = rustix::system::reboot(rustix::system::RebootCommand::CadOff);
    pid1::seed_etc(Path::new("/"), &mut log);
    pid1::mount_system_volumes(&cmdline, &mut log);
    pid1::set_hostname(&mut log);
    pid1::load_random_seed();
    pid1::apply_sysctl(&mut log);
    solinit::kmod::load_configured(&mut log);
    if let Err(e) = log.persist_to(Path::new("/").join(LOG_FILE).as_path()) {
        eprintln!("solace: log store: {e}");
    }
    match Runtime::new(Path::new("/"), Mode::Pid1, log) {
        Ok(mut rt) => {
            let p = rt.run();
            pid1::finish(p, &mut rt.log)
        }
        Err(e) => {
            eprintln!("solace: cannot start services: {e}");
            emergency()
        }
    }
}

fn initramfs(c: &Cmdline) -> ! {
    let Some(spec) = c.root.as_deref() else {
        eprintln!("solace: no sol.root= on the kernel command line");
        emergency()
    };
    let Some(dev) = pid1::find_root(spec) else {
        eprintln!("solace: root device {spec} did not appear");
        emergency()
    };
    let system = c.system.as_deref().unwrap_or("@system");
    let new_root = Path::new("/sysroot");
    if let Err(e) = pid1::assemble_root(&dev, system, new_root) {
        eprintln!("solace: {e}");
        emergency()
    }
    let e = pid1::switch_root(new_root);
    eprintln!("solace: could not start {}: {e}", pid1::INIT_PATH);
    emergency()
}

/// Something essential failed: say so and wait, rather than panic (a PID 1
/// panic is a kernel panic with an unreadable message).
fn emergency() -> ! {
    eprintln!("solace: boot failed; the system is halted. Press the power button to turn off.");
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
