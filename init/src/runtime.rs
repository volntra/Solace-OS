//! Carries out the [`Manager`]'s decisions: spawns and supervises service
//! processes, reaps children, captures output into sol-log, applies
//! cgroup limits, and serves the solctl control socket.
//!
//! The same loop runs as PID 1 and, for development and tests, as an
//! ordinary process managing a scratch root (`sol-init --root DIR`).

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixDatagram, UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{ChildStderr, ChildStdout, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, poll};
use rustix::process::{Pid, Signal, WaitOptions, kill_process, waitpid};

use crate::log::{LineBuffer, LogStore};
use crate::manager::{Action, Manager, Sig};
use crate::policy::Exit;
use crate::proto::{self, Request, Response};
use crate::unit::{self, DISABLED_DIR, ENABLED_DIR, ServiceType, Unit};

pub const CGROUP_ROOT: &str = "sys/fs/cgroup";
const NOTIFY_DIR: &str = "run/sol/notify";
/// Sent by UPS/power daemons and some hypervisors (Linux x86-64 value;
/// not exported by signal-hook).
const SIGPWR: i32 = 30;
const DEFAULT_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin";

/// What the system should do once the loop ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Power {
    Reboot,
    Poweroff,
    /// Non-PID-1 mode: just exit.
    Exit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Running as PID 1: root-only control socket, cgroups, reboots.
    Pid1,
    /// A supervisor for a scratch root (development and tests). The control
    /// socket accepts the invoking user; SIGTERM/SIGINT shut down and exit.
    User,
}

struct Proc {
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
    out_buf: LineBuffer,
    err_buf: LineBuffer,
    notify: Option<UnixDatagram>,
}

struct Client {
    stream: UnixStream,
    buf: Vec<u8>,
}

pub struct Runtime {
    root: PathBuf,
    mode: Mode,
    mgr: Manager,
    pub log: LogStore,
    procs: HashMap<String, Proc>,
    listener: UnixListener,
    clients: Vec<Client>,
    sig_rx: UnixStream,
    last_signal: Arc<AtomicUsize>,
    started: Instant,
    power: Option<Power>,
    cgroups: Option<PathBuf>,
    boot_logged: bool,
    /// `CLOCK_BOOTTIME` (ms since the kernel started) when the loop began.
    pub boottime_at_start: u64,
}

fn boottime_ms() -> u64 {
    let ts = rustix::time::clock_gettime(rustix::time::ClockId::Boottime);
    u64::try_from(ts.tv_sec).unwrap_or(0) * 1000
        + u64::try_from(ts.tv_nsec).unwrap_or(0) / 1_000_000
}

impl Runtime {
    /// Loads units from `root`, opens the control socket, and installs
    /// signal handlers.
    ///
    /// # Errors
    /// Unit errors (malformed units, cycles) and I/O errors.
    pub fn new(root: &Path, mode: Mode, log: LogStore) -> crate::Result<Self> {
        let units = unit::load_all(root)?;
        let enabled = unit::enabled_set(root, &units);
        let mgr = Manager::new(units, enabled)?;

        let sol_run = root.join("run/sol");
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&sol_run)
            .map_err(|e| crate::Error::io(&sol_run, &e))?;
        let sock = root.join(proto::SOCKET);
        let _ = fs::remove_file(&sock);
        let listener = UnixListener::bind(&sock).map_err(|e| crate::Error::io(&sock, &e))?;
        fs::set_permissions(&sock, fs::Permissions::from_mode(0o600))
            .map_err(|e| crate::Error::io(&sock, &e))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| crate::Error::io(&sock, &e))?;

        let (sig_rx, sig_tx) = UnixStream::pair().map_err(|e| crate::Error::io(root, &e))?;
        sig_rx
            .set_nonblocking(true)
            .map_err(|e| crate::Error::io(root, &e))?;
        let last_signal = Arc::new(AtomicUsize::new(0));
        for sig in [
            signal_hook::consts::SIGCHLD,
            signal_hook::consts::SIGTERM,
            signal_hook::consts::SIGINT,
            signal_hook::consts::SIGUSR1,
            signal_hook::consts::SIGUSR2,
            SIGPWR,
        ] {
            let tx = sig_tx.try_clone().map_err(|e| crate::Error::io(root, &e))?;
            signal_hook::low_level::pipe::register(sig, tx)
                .map_err(|e| crate::Error::io(root, &e))?;
            if sig != signal_hook::consts::SIGCHLD {
                #[allow(clippy::cast_sign_loss)]
                signal_hook::flag::register_usize(sig, Arc::clone(&last_signal), sig as usize)
                    .map_err(|e| crate::Error::io(root, &e))?;
            }
        }

        let cg = root.join(CGROUP_ROOT);
        let cgroups = (mode == Mode::Pid1 && cg.join("cgroup.controllers").exists()).then(|| {
            let sol = cg.join("sol");
            let _ = fs::create_dir_all(&sol);
            // Delegate the controllers our limits use.
            let _ = fs::write(cg.join("cgroup.subtree_control"), "+memory +pids +cpu");
            sol
        });
        if let Some(sol) = &cgroups {
            let _ = fs::write(sol.join("cgroup.subtree_control"), "+memory +pids +cpu");
        }

        Ok(Runtime {
            root: root.to_path_buf(),
            mode,
            mgr,
            log,
            procs: HashMap::new(),
            listener,
            clients: Vec::new(),
            sig_rx,
            last_signal,
            started: Instant::now(),
            power: None,
            cgroups,
            boot_logged: false,
            boottime_at_start: boottime_ms(),
        })
    }

    fn now(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn info(&mut self, msg: &str) {
        self.log.push("sol-init", "init", msg);
    }

    /// Runs until a shutdown has completed.
    pub fn run(&mut self) -> Power {
        let now = self.now();
        self.mgr.boot(now);
        self.info("starting boot units");
        loop {
            let now = self.now();
            let actions = self.mgr.tick(now);
            self.execute(actions);
            self.reap();
            self.check_boot_done();
            if let Some(p) = self.power
                && self.mgr.shutdown_complete()
            {
                self.info("all units stopped");
                self.log.flush();
                return p;
            }
            self.wait_for_events();
        }
    }

    fn check_boot_done(&mut self) {
        if self.boot_logged {
            return;
        }
        if let Some(at) = self.mgr.boot_done_at() {
            self.boot_logged = true;
            let since_kernel = self.boottime_at_start + at;
            let msg = format!(
                "boot complete: {since_kernel} ms after the kernel started ({at} ms in sol-init)"
            );
            self.info(&msg);
            if self.mode == Mode::Pid1 {
                eprintln!("solace: {msg}");
            }
            let _ = fs::write(
                self.root.join("run/sol/boot-time"),
                format!("{since_kernel}\n"),
            );
        }
    }

    fn execute(&mut self, actions: Vec<Action>) {
        for a in actions {
            match a {
                Action::Spawn(name) => {
                    let now = self.now();
                    match self.spawn(&name) {
                        Ok(pid) => {
                            self.mgr.spawned(&name, pid, now);
                            self.log
                                .push(&name, "init", &format!("started (pid {pid})"));
                        }
                        Err(e) => {
                            self.log
                                .push(&name, "init", &format!("could not start: {e}"));
                            self.mgr.spawn_failed(&name, &e, now);
                        }
                    }
                }
                Action::Signal { unit, pid, sig } => self.signal(&unit, pid, sig),
                Action::Log { unit, msg } => self.log.push(&unit, "init", &msg),
            }
        }
    }

    fn spawn(&mut self, name: &str) -> Result<u32, String> {
        let u: Unit = self
            .mgr
            .unit(name)
            .cloned()
            .ok_or_else(|| "unknown unit".to_string())?;
        let svc = &u.service;
        let mut cmd = Command::new(&svc.exec[0]);
        cmd.args(&svc.exec[1..])
            .env_clear()
            .env("PATH", DEFAULT_PATH)
            .env("SOL_UNIT", name)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(svc.working_dir.as_deref().unwrap_or(Path::new("/")));
        for (k, v) in &svc.environment {
            cmd.env(k, v);
        }
        if let Some(user) = &svc.user {
            let (uid, gid) = lookup_user(&self.root, user)?;
            cmd.uid(uid).gid(gid);
            cmd.env("USER", user);
        }
        if let Some(group) = &svc.group {
            cmd.gid(lookup_group(&self.root, group)?);
        }
        let notify = if svc.kind == ServiceType::Notify {
            let dir = self.root.join(NOTIFY_DIR);
            fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let path = dir.join(format!("{name}.sock"));
            let _ = fs::remove_file(&path);
            let s = UnixDatagram::bind(&path).map_err(|e| e.to_string())?;
            s.set_nonblocking(true).map_err(|e| e.to_string())?;
            cmd.env("NOTIFY_SOCKET", &path);
            Some(s)
        } else {
            None
        };
        if u.sandbox.no_new_privs {
            // SAFETY: the closure runs in the forked child before exec and
            // only makes one prctl(2) system call, which is
            // async-signal-safe; it touches no locks or allocations.
            #[allow(unsafe_code)]
            unsafe {
                cmd.pre_exec(|| {
                    rustix::thread::set_no_new_privs(true).map_err(std::io::Error::from)
                });
            }
        }
        let mut child = cmd.spawn().map_err(|e| format!("{}: {e}", svc.exec[0]))?;
        let pid = child.id();
        self.place_in_cgroup(&u, pid);
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        for fd in [
            stdout.as_ref().map(AsFd::as_fd),
            stderr.as_ref().map(AsFd::as_fd),
        ]
        .into_iter()
        .flatten()
        {
            set_nonblocking(fd);
        }
        // The Child handle is dropped: we reap with waitpid(-1) ourselves.
        drop(child);
        self.procs.insert(
            name.to_string(),
            Proc {
                stdout,
                stderr,
                out_buf: LineBuffer::default(),
                err_buf: LineBuffer::default(),
                notify,
            },
        );
        Ok(pid)
    }

    fn place_in_cgroup(&mut self, u: &Unit, pid: u32) {
        let Some(base) = &self.cgroups else { return };
        let cg = base.join(&u.name);
        if fs::create_dir_all(&cg).is_err() {
            return;
        }
        let mut errs = Vec::new();
        if let Ok(Some(m)) = u.limits.memory_max_bytes()
            && let Err(e) = fs::write(cg.join("memory.max"), m.to_string())
        {
            errs.push(format!("memory.max: {e}"));
        }
        if let Some(t) = u.limits.tasks_max
            && let Err(e) = fs::write(cg.join("pids.max"), t.to_string())
        {
            errs.push(format!("pids.max: {e}"));
        }
        if let Some(w) = u.limits.cpu_weight
            && let Err(e) = fs::write(cg.join("cpu.weight"), w.to_string())
        {
            errs.push(format!("cpu.weight: {e}"));
        }
        if let Err(e) = fs::write(cg.join("cgroup.procs"), pid.to_string()) {
            errs.push(format!("cgroup.procs: {e}"));
        }
        for e in errs {
            self.log.push(&u.name, "init", &format!("cgroup: {e}"));
        }
    }

    fn signal(&mut self, unit: &str, pid: u32, sig: Sig) {
        let cg = self.cgroups.as_ref().map(|b| b.join(unit));
        if sig == Sig::Kill
            && let Some(cg) = &cg
            && fs::write(cg.join("cgroup.kill"), "1").is_ok()
        {
            return;
        }
        let signal = match sig {
            Sig::Term => Signal::TERM,
            Sig::Kill => Signal::KILL,
        };
        // Signal every process in the unit's cgroup, else just the main pid.
        let mut pids: Vec<u32> = cg
            .and_then(|c| fs::read_to_string(c.join("cgroup.procs")).ok())
            .map(|t| t.lines().filter_map(|l| l.trim().parse().ok()).collect())
            .unwrap_or_default();
        if !pids.contains(&pid) {
            pids.push(pid);
        }
        for p in pids {
            if let Some(p) = i32::try_from(p).ok().and_then(Pid::from_raw) {
                let _ = kill_process(p, signal);
            }
        }
    }

    fn reap(&mut self) {
        loop {
            match waitpid(None, WaitOptions::NOHANG) {
                Ok(Some((pid, status))) => {
                    let exit = if let Some(code) = status.exit_status() {
                        Exit::Code(code)
                    } else if let Some(sig) = status.terminating_signal() {
                        Exit::Signal(sig)
                    } else {
                        continue;
                    };
                    let pid = u32::try_from(pid.as_raw_nonzero().get()).unwrap_or(0);
                    let unit = self
                        .mgr
                        .pids()
                        .into_iter()
                        .find(|(_, p)| *p == pid)
                        .map(|(n, _)| n);
                    if let Some(u) = &unit {
                        self.drain_proc(u, true);
                        self.procs.remove(u);
                        if let Some(base) = &self.cgroups {
                            let _ = fs::remove_dir(base.join(u));
                        }
                    }
                    let now = self.now();
                    let acts = self.mgr.exited(pid, exit, now);
                    self.execute(acts);
                }
                Ok(None) | Err(_) => return,
            }
        }
    }

    /// Reads whatever output a unit has produced into sol-log.
    fn drain_proc(&mut self, unit: &str, closing: bool) {
        let Some(p) = self.procs.get_mut(unit) else {
            return;
        };
        let mut lines: Vec<(&'static str, String)> = Vec::new();
        let mut buf = [0u8; 8192];
        for (stream, reader, lb) in [
            (
                "out",
                p.stdout.as_mut().map(|s| s as &mut dyn Read),
                &mut p.out_buf,
            ),
            (
                "err",
                p.stderr.as_mut().map(|s| s as &mut dyn Read),
                &mut p.err_buf,
            ),
        ] {
            let Some(r) = reader else { continue };
            loop {
                match r.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => lines.extend(lb.feed(&buf[..n]).into_iter().map(|l| (stream, l))),
                    Err(e) if e.kind() == ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
            if closing && let Some(rest) = lb.finish() {
                lines.push((stream, rest));
            }
        }
        let mut ready = false;
        if let Some(n) = &p.notify {
            let mut b = [0u8; 4096];
            while let Ok(k) = n.recv(&mut b) {
                if String::from_utf8_lossy(&b[..k])
                    .lines()
                    .any(|l| l.trim() == "READY=1")
                {
                    ready = true;
                }
            }
        }
        for (stream, l) in lines {
            self.log.push(unit, stream, &l);
        }
        if ready {
            let now = self.now();
            self.mgr.ready(unit, now);
            self.log.push(unit, "init", "ready");
        }
    }

    fn wait_for_events(&mut self) {
        let now = self.now();
        let timeout_ms = self
            .mgr
            .next_deadline()
            .map_or(1000, |d| d.saturating_sub(now).min(1000));
        // Collect raw fds first so the poll set does not borrow self.
        let mut watched: Vec<(Source, i32)> = vec![
            (Source::Signals, self.sig_rx.as_raw_fd()),
            (Source::Listener, self.listener.as_raw_fd()),
        ];
        for (i, c) in self.clients.iter().enumerate() {
            watched.push((Source::Client(i), c.stream.as_raw_fd()));
        }
        for (name, p) in &self.procs {
            for fd in [
                p.stdout.as_ref().map(AsRawFd::as_raw_fd),
                p.stderr.as_ref().map(AsRawFd::as_raw_fd),
                p.notify.as_ref().map(AsRawFd::as_raw_fd),
            ]
            .into_iter()
            .flatten()
            {
                watched.push((Source::Proc(name.clone()), fd));
            }
        }
        let ready: Vec<Source> = {
            // SAFETY: every fd in `watched` belongs to an object owned by
            // `self`, which is not modified until the poll returns.
            #[allow(unsafe_code)]
            let borrowed: Vec<BorrowedFd<'_>> = watched
                .iter()
                .map(|(_, fd)| unsafe { BorrowedFd::borrow_raw(*fd) })
                .collect();
            let mut fds: Vec<PollFd<'_>> = borrowed
                .iter()
                .map(|fd| PollFd::new(fd, PollFlags::IN))
                .collect();
            let ts = rustix::time::Timespec {
                tv_sec: i64::try_from(timeout_ms / 1000).unwrap_or(1),
                tv_nsec: i64::try_from((timeout_ms % 1000) * 1_000_000).unwrap_or(0),
            };
            if poll(&mut fds, Some(&ts)).is_err() {
                return;
            }
            fds.iter()
                .zip(&watched)
                .filter(|(f, _)| !f.revents().is_empty())
                .map(|(_, (s, _))| s.clone())
                .collect()
        };
        let mut dead_clients = Vec::new();
        for s in ready {
            match s {
                Source::Signals => self.on_signals(),
                Source::Listener => self.accept(),
                Source::Client(i) => {
                    if self.serve_client(i) {
                        dead_clients.push(i);
                    }
                }
                Source::Proc(name) => self.drain_proc(&name, false),
            }
        }
        dead_clients.sort_unstable();
        dead_clients.dedup();
        for i in dead_clients.into_iter().rev() {
            self.clients.remove(i);
        }
    }

    fn on_signals(&mut self) {
        let mut b = [0u8; 64];
        while matches!(self.sig_rx.read(&mut b), Ok(n) if n > 0) {}
        let sig = self.last_signal.swap(0, Ordering::SeqCst);
        if sig == 0 {
            return; // SIGCHLD: reaped on the next loop turn
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let sig = sig as i32;
        let power = match (self.mode, sig) {
            (Mode::User, _) => Power::Exit,
            (Mode::Pid1, s) if s == signal_hook::consts::SIGUSR2 || s == SIGPWR => Power::Poweroff,
            // SIGINT is Ctrl-Alt-Del (we turn the kernel's own handling off).
            (Mode::Pid1, _) => Power::Reboot,
        };
        self.request_power(power);
    }

    fn request_power(&mut self, p: Power) {
        if self.power.is_none() {
            self.power = Some(p);
            self.info(&format!("shutting down ({p:?})"));
            let now = self.now();
            let acts = self.mgr.begin_shutdown(now);
            self.execute(acts);
        }
    }

    fn accept(&mut self) {
        while let Ok((stream, _)) = self.listener.accept() {
            if stream.set_nonblocking(true).is_ok() && self.clients.len() < 64 {
                self.clients.push(Client {
                    stream,
                    buf: Vec::new(),
                });
            }
        }
    }

    /// Returns true when the client is finished (answered or broken).
    fn serve_client(&mut self, i: usize) -> bool {
        let mut b = [0u8; 4096];
        let c = &mut self.clients[i];
        match c.stream.read(&mut b) {
            Ok(0) => return true,
            Ok(n) => c.buf.extend_from_slice(&b[..n]),
            Err(e) if e.kind() == ErrorKind::WouldBlock => return false,
            Err(_) => return true,
        }
        if c.buf.len() > proto::MAX_REQUEST {
            return true;
        }
        let Some(nl) = c.buf.iter().position(|x| *x == b'\n') else {
            return false;
        };
        let line = String::from_utf8_lossy(&c.buf[..nl]).into_owned();
        let peer_uid = rustix::net::sockopt::socket_peercred(&c.stream)
            .map(|cred| cred.uid.as_raw())
            .ok();
        let resp = match proto::parse_request(&line) {
            Err(e) => Response::Error { message: e },
            Ok(req) => {
                let allowed = match self.mode {
                    Mode::Pid1 => peer_uid == Some(0),
                    Mode::User => peer_uid == Some(rustix::process::geteuid().as_raw()),
                };
                if req.mutates() && !allowed {
                    Response::Error {
                        message: "permission denied".into(),
                    }
                } else {
                    self.handle(req)
                }
            }
        };
        let out = proto::encode(&resp);
        let c = &mut self.clients[i];
        let _ = c.stream.set_nonblocking(false);
        let _ = c.stream.set_write_timeout(Some(Duration::from_secs(2)));
        let _ = c.stream.write_all(out.as_bytes());
        true
    }

    fn handle(&mut self, req: Request) -> Response {
        let now = self.now();
        let ok = |m: String| Response::Ok { message: m };
        let err = |m: String| Response::Error { message: m };
        match req {
            Request::Status { unit } => {
                let mut units = self.mgr.status(now);
                if let Some(u) = unit {
                    units.retain(|s| s.name == u);
                    if units.is_empty() {
                        return err(format!("no unit named {u}"));
                    }
                }
                Response::Status { units }
            }
            Request::Start { unit } => match self.mgr.start(&unit, now) {
                Ok(()) => ok(format!("starting {unit}")),
                Err(e) => err(e.to_string()),
            },
            Request::Stop { unit } => match self.mgr.stop(&unit, now) {
                Ok(a) => {
                    self.execute(a);
                    ok(format!("stopping {unit}"))
                }
                Err(e) => err(e.to_string()),
            },
            Request::Restart { unit } => match self.mgr.restart(&unit, now) {
                Ok(a) => {
                    self.execute(a);
                    ok(format!("restarting {unit}"))
                }
                Err(e) => err(e.to_string()),
            },
            Request::Enable { unit } | Request::Disable { unit }
                if self.mgr.unit(&unit).is_none() =>
            {
                err(format!("no unit named {unit}"))
            }
            Request::Enable { unit } => match self.set_enabled(&unit, true) {
                Ok(()) => ok(format!("{unit} will start at boot")),
                Err(e) => err(e),
            },
            Request::Disable { unit } => match self.set_enabled(&unit, false) {
                Ok(()) => ok(format!("{unit} will not start at boot")),
                Err(e) => err(e),
            },
            Request::Logs { unit, lines } => Response::Logs {
                records: self.log.tail(unit.as_deref(), lines.min(10_000)),
            },
            Request::BootTime => match self.mgr.boot_done_at() {
                Some(at) => ok(format!(
                    "{} ms after the kernel started ({at} ms in sol-init)",
                    self.boottime_at_start + at
                )),
                None => ok("boot still in progress".into()),
            },
            Request::Reboot => {
                self.request_power(Power::Reboot);
                ok("rebooting".into())
            }
            Request::Poweroff => {
                self.request_power(if self.mode == Mode::Pid1 {
                    Power::Poweroff
                } else {
                    Power::Exit
                });
                ok("powering off".into())
            }
        }
    }

    fn set_enabled(&mut self, unit: &str, on: bool) -> Result<(), String> {
        let (add, del) = if on {
            (ENABLED_DIR, DISABLED_DIR)
        } else {
            (DISABLED_DIR, ENABLED_DIR)
        };
        let dir = self.root.join(add);
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        fs::write(dir.join(unit), "").map_err(|e| e.to_string())?;
        let _ = fs::remove_file(self.root.join(del).join(unit));
        self.mgr.set_enabled(unit, on);
        Ok(())
    }

    /// Units and their states (for tests).
    pub fn snapshot(&self) -> BTreeMap<String, String> {
        self.mgr
            .status(self.now())
            .into_iter()
            .map(|s| (s.name, s.state))
            .collect()
    }
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime")
            .field("root", &self.root)
            .field("mode", &self.mode)
            .field("units", &self.snapshot())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug)]
enum Source {
    Signals,
    Listener,
    Client(usize),
    Proc(String),
}

fn set_nonblocking(fd: BorrowedFd<'_>) {
    if let Ok(flags) = rustix::fs::fcntl_getfl(fd) {
        let _ = rustix::fs::fcntl_setfl(fd, flags | rustix::fs::OFlags::NONBLOCK);
    }
}

/// Resolves a user name (or numeric uid) via `<root>/etc/passwd`.
fn lookup_user(root: &Path, user: &str) -> Result<(u32, u32), String> {
    if let Ok(uid) = user.parse::<u32>() {
        return Ok((uid, uid));
    }
    let text = fs::read_to_string(root.join("etc/passwd")).unwrap_or_default();
    text.lines()
        .map(|l| l.split(':').collect::<Vec<_>>())
        .find(|f| f.len() >= 4 && f[0] == user)
        .and_then(|f| Some((f[2].parse().ok()?, f[3].parse().ok()?)))
        .ok_or_else(|| format!("unknown user {user}"))
}

fn lookup_group(root: &Path, group: &str) -> Result<u32, String> {
    if let Ok(gid) = group.parse::<u32>() {
        return Ok(gid);
    }
    let text = fs::read_to_string(root.join("etc/group")).unwrap_or_default();
    text.lines()
        .map(|l| l.split(':').collect::<Vec<_>>())
        .find(|f| f.len() >= 3 && f[0] == group)
        .and_then(|f| f[2].parse().ok())
        .ok_or_else(|| format!("unknown group {group}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_lookup() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir_all(d.path().join("etc")).unwrap();
        fs::write(
            d.path().join("etc/passwd"),
            "root:x:0:0::/root:/bin/sh\nkoi:x:1000:100::/Users/koi:/bin/ace\n",
        )
        .unwrap();
        fs::write(d.path().join("etc/group"), "root:x:0:\nusers:x:100:\n").unwrap();
        assert_eq!(lookup_user(d.path(), "koi").unwrap(), (1000, 100));
        assert_eq!(lookup_user(d.path(), "42").unwrap(), (42, 42));
        assert!(lookup_user(d.path(), "nobody").is_err());
        assert_eq!(lookup_group(d.path(), "users").unwrap(), 100);
        assert!(lookup_group(d.path(), "wheel").is_err());
    }
}
