//! fern-settingsd: serves the user's settings on the session bus, or with
//! `--system` the system's on the system bus (docs/protocols/settings.md).

use std::path::PathBuf;
use std::process::ExitCode;

use settingsd::bus::{self, Bus, Settings};
use settingsd::schema::Scope;
use settingsd::store::Store;

const USAGE: &str = "usage: fern-settingsd [--system] [--address ADDR] [--dir DIR] [--boot-hook CMD | --no-boot-hook]
  --system      the system's settings on the system bus (default: the user's
                on the session bus)
  --address     serve on this bus address instead
  --dir         where the TOML files live (default ~/.config/fern, or
                /etc/fern with --system)
  --boot-hook   run after the boot settings change; a failure undoes the
                change (default with --system: /usr/bin/spk boot-menu)";

fn main() -> ExitCode {
    let mut scope = Scope::User;
    let mut address = None;
    let mut dir = None;
    let mut hook: Option<Option<String>> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--system" => scope = Scope::System,
            "--address" => address = args.next(),
            "--dir" => dir = args.next().map(PathBuf::from),
            "--boot-hook" => hook = Some(args.next()),
            "--no-boot-hook" => hook = Some(None),
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let dir = match dir {
        Some(d) => d,
        None if scope == Scope::System => PathBuf::from("/etc/fern"),
        None => {
            let config = std::env::var_os("XDG_CONFIG_HOME")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
            let Some(config) = config else {
                eprintln!("fern-settingsd: neither XDG_CONFIG_HOME nor HOME is set");
                return ExitCode::FAILURE;
            };
            config.join("fern")
        }
    };
    let hook = match hook {
        Some(h) => h,
        None if scope == Scope::System => Some("/usr/bin/spk boot-menu".to_owned()),
        None => None,
    }
    .map(|h| h.split_whitespace().map(str::to_owned).collect::<Vec<_>>())
    .filter(|h| !h.is_empty());
    let bus = match (address, scope) {
        (Some(a), _) => Bus::Address(a),
        (None, Scope::System) => Bus::System,
        (None, Scope::User) => Bus::Session,
    };

    let (store, warnings) = Store::open(scope, &dir);
    for w in warnings {
        eprintln!("fern-settingsd: {w}");
    }
    zbus::block_on(async {
        // The bus may still be starting (both start at boot or login).
        let mut tries = 0;
        let conn = loop {
            match bus::connect(&bus).await {
                Ok(c) => break c,
                Err(e) if tries < 50 => {
                    tries += 1;
                    if tries == 1 {
                        eprintln!("fern-settingsd: waiting for the bus ({e})");
                    }
                    // Only this task waits; the bus runs on its own thread.
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                Err(e) => {
                    eprintln!("fern-settingsd: {e}");
                    return ExitCode::FAILURE;
                }
            }
        };
        if let Err(e) = bus::serve(&conn, Settings::new(store, hook)).await {
            eprintln!("fern-settingsd: {e}");
            return ExitCode::FAILURE;
        }
        let what = if scope == Scope::System {
            "system"
        } else {
            "user"
        };
        eprintln!(
            "fern-settingsd: serving the {what} settings in {}",
            dir.display()
        );
        std::future::pending::<ExitCode>().await
    })
}
