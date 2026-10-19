//! solctl: control sol-init.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use solinit::proto::{self, Request, Response};

const USAGE: &str = "usage: solctl [--root DIR] <command>

  status [UNIT]        show units (or one unit)
  start UNIT           start a unit and its requirements
  stop UNIT            stop a unit
  restart UNIT         stop and start a unit (clears its crash history)
  enable UNIT          start the unit at boot
  disable UNIT         do not start the unit at boot
  logs [UNIT] [-n N]   show the last N log lines (default 50)
  boot-time            how long the last boot took
  reboot | poweroff";

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut root = PathBuf::from("/");
    if args.first().map(String::as_str) == Some("--root") {
        if args.len() < 2 {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
        root = PathBuf::from(args.remove(1));
        args.remove(0);
    }
    let req = match parse(&args) {
        Ok(r) => r,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("solctl: {e}");
            }
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let sock = root.join(proto::SOCKET);
    let resp = match send(&sock, &req) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("solctl: cannot reach sol-init at {}: {e}", sock.display());
            return ExitCode::FAILURE;
        }
    };
    print(resp)
}

fn parse(args: &[String]) -> Result<Request, String> {
    let unit = |i: usize| {
        args.get(i)
            .cloned()
            .ok_or_else(|| format!("{} needs a unit name", args[0]))
    };
    let Some(cmd) = args.first() else {
        return Err(String::new());
    };
    Ok(match cmd.as_str() {
        "status" => Request::Status {
            unit: args.get(1).cloned(),
        },
        "start" => Request::Start { unit: unit(1)? },
        "stop" => Request::Stop { unit: unit(1)? },
        "restart" => Request::Restart { unit: unit(1)? },
        "enable" => Request::Enable { unit: unit(1)? },
        "disable" => Request::Disable { unit: unit(1)? },
        "logs" => {
            let mut unit = None;
            let mut lines = 50;
            let mut it = args[1..].iter();
            while let Some(a) = it.next() {
                if a == "-n" {
                    lines = it
                        .next()
                        .and_then(|n| n.parse().ok())
                        .ok_or("-n needs a number")?;
                } else {
                    unit = Some(a.clone());
                }
            }
            Request::Logs { unit, lines }
        }
        "boot-time" => Request::BootTime,
        "reboot" => Request::Reboot,
        "poweroff" => Request::Poweroff,
        "-h" | "--help" | "help" => return Err(String::new()),
        other => return Err(format!("unknown command {other}")),
    })
}

fn send(sock: &std::path::Path, req: &Request) -> std::io::Result<Response> {
    let mut s = UnixStream::connect(sock)?;
    s.set_read_timeout(Some(Duration::from_secs(10)))?;
    s.write_all(proto::encode(req).as_bytes())?;
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line)?;
    serde_json::from_str(&line).map_err(std::io::Error::other)
}

fn print(resp: Response) -> ExitCode {
    match resp {
        Response::Ok { message } => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Response::Error { message } => {
            eprintln!("solctl: {message}");
            ExitCode::FAILURE
        }
        Response::Status { units } => {
            let w = units.iter().map(|u| u.name.len()).max().unwrap_or(4).max(4);
            println!(
                "{:<w$}  {:<10}  {:>8}  {:>7}  DESCRIPTION",
                "UNIT", "STATE", "PID", "FOR"
            );
            for u in units {
                let pid = u.pid.map_or_else(|| "-".into(), |p| p.to_string());
                let (state, detail) = match u.state.split_once(": ") {
                    Some((s, d)) => (s.to_string(), format!(" ({d})")),
                    None => (u.state.clone(), String::new()),
                };
                let mark = if u.enabled { "" } else { " [not enabled]" };
                println!(
                    "{:<w$}  {:<10}  {:>8}  {:>7}  {}{detail}{mark}",
                    u.name,
                    state,
                    pid,
                    human(u.for_ms),
                    u.description
                );
            }
            ExitCode::SUCCESS
        }
        Response::Logs { records } => {
            for r in records {
                let secs = r.t / 1000;
                let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
                println!("{h:02}:{m:02}:{s:02} {:<16} {}", r.unit, r.msg);
            }
            ExitCode::SUCCESS
        }
    }
}

fn human(ms: u64) -> String {
    match ms {
        0..=999 => format!("{ms}ms"),
        1000..=59_999 => format!("{}s", ms / 1000),
        60_000..=3_599_999 => format!("{}m", ms / 60_000),
        _ => format!("{}h", ms / 3_600_000),
    }
}
