//! sol-bootgen CLI.

use std::path::PathBuf;
use std::process::ExitCode;

use bootgen::{Inventory, Params, Settings};

const USAGE: &str = "usage: sol-bootgen --esp <dir> --inventory <systems.toml> \
[--settings <boot.toml>] [--dry-run]

  --esp        the mounted EFI system partition
  --inventory  bootable systems and other OSes (see docs/formats/bootgen.md)
  --settings   the boot settings domain (default: built-in defaults)
  --dry-run    print grub.cfg instead of changing the ESP";

fn main() -> ExitCode {
    let mut esp = None;
    let mut inv = None;
    let mut settings = None;
    let mut dry = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--esp" => esp = args.next().map(PathBuf::from),
            "--inventory" => inv = args.next().map(PathBuf::from),
            "--settings" => settings = args.next().map(PathBuf::from),
            "--dry-run" => dry = true,
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(esp), Some(inv)) = (esp, inv) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    match run(&esp, &inv, settings.as_deref(), dry) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("sol-bootgen: {e}");
            ExitCode::FAILURE
        }
    }
}

fn read_toml<T: serde::de::DeserializeOwned>(p: &std::path::Path) -> Result<T, String> {
    let text = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))
}

fn run(
    esp: &std::path::Path,
    inv: &std::path::Path,
    settings: Option<&std::path::Path>,
    dry: bool,
) -> Result<(), String> {
    let inv: Inventory = read_toml(inv)?;
    let settings: Settings = match settings {
        Some(p) => read_toml(p)?,
        None => Settings::default(),
    };
    let params = Params {
        esp_uuid: inv.esp_uuid,
        root_uuid: inv.root_uuid,
        systems: inv.systems,
        other_os: inv.other_os,
        settings,
        os_name: inv.os_name,
    };
    if dry {
        print!(
            "{}",
            bootgen::generate_cfg(&params).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    let r = bootgen::apply(esp, &params, &bootgen::free_bytes).map_err(|e| e.to_string())?;
    println!(
        "sol-bootgen: {} boot entries; copied {}; removed {}",
        r.entries,
        if r.copied.is_empty() {
            "nothing".to_string()
        } else {
            r.copied.join(", ")
        },
        if r.removed.is_empty() {
            "nothing".to_string()
        } else {
            r.removed.join(", ")
        }
    );
    Ok(())
}
