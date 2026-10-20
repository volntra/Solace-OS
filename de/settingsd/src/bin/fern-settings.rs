//! fern-settings: read, change and watch settings through fern-settingsd.

use std::process::ExitCode;

use settingsd::Value;
use settingsd::bus::{INTERFACE, NAME, PATH};
use zbus::blocking::{Connection, MessageIterator};
use zbus::zvariant::OwnedValue;
use zbus::{MatchRule, message};

const USAGE: &str = "usage: fern-settings [--system] [--address ADDR] COMMAND
  list [DOMAIN]             domains, or a domain's keys and values
  describe DOMAIN           a domain's keys: type, default and meaning
  get DOMAIN KEY
  set DOMAIN KEY VALUE      lists are comma-separated
  reset DOMAIN KEY          back to the default
  watch [DOMAIN]            print changes as they happen
  --system   the system's settings (boot) instead of the user's";

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut system = false;
    let mut address = None;
    while let Some(a) = args.first().cloned() {
        match a.as_str() {
            "--system" => {
                system = true;
                args.remove(0);
            }
            "--address" if args.len() > 1 => {
                address = Some(args[1].clone());
                args.drain(..2);
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            _ => break,
        }
    }
    match run(system, address.as_deref(), &args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Problem::Usage) => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
        Err(Problem::Failed(e)) => {
            eprintln!("fern-settings: {e}");
            ExitCode::FAILURE
        }
    }
}

enum Problem {
    Usage,
    Failed(String),
}

impl From<zbus::Error> for Problem {
    fn from(e: zbus::Error) -> Problem {
        Problem::Failed(match e {
            zbus::Error::MethodError(_, Some(m), _) => m,
            e => e.to_string(),
        })
    }
}

enum Args<'a> {
    None,
    One(&'a str),
    Two(&'a str, &'a str),
    Set(&'a str, &'a str, zbus::zvariant::Value<'a>),
}

type Described = Vec<(String, String, String, OwnedValue)>;

fn run(system: bool, address: Option<&str>, args: &[String]) -> Result<(), Problem> {
    let conn = match (address, system) {
        (Some(a), _) => zbus::blocking::connection::Builder::address(a)?.build()?,
        (None, true) => Connection::system()?,
        (None, false) => Connection::session()?,
    };
    let call = |method: &str, body: &Args<'_>| match body {
        Args::None => conn.call_method(Some(NAME), PATH, Some(INTERFACE), method, &()),
        Args::One(a) => conn.call_method(Some(NAME), PATH, Some(INTERFACE), method, &(a,)),
        Args::Two(a, b) => conn.call_method(Some(NAME), PATH, Some(INTERFACE), method, &(a, b)),
        Args::Set(a, b, v) => {
            conn.call_method(Some(NAME), PATH, Some(INTERFACE), method, &(a, b, v))
        }
    };
    let s = |i: usize| args.get(i).map(String::as_str);
    match (s(0), s(1), s(2), s(3)) {
        (Some("list"), None, None, None) => {
            let domains: Vec<String> = call("Domains", &Args::None)?.body().deserialize()?;
            for d in domains {
                println!("{d}");
            }
        }
        (Some("list"), Some(d), None, None) => {
            let keys: Described = call("Describe", &Args::One(d))?.body().deserialize()?;
            for (k, _, _, _) in keys {
                let v: OwnedValue = call("Get", &Args::Two(d, &k))?.body().deserialize()?;
                println!("{k} = {}", show(&v));
            }
        }
        (Some("describe"), Some(d), None, None) => {
            let keys: Described = call("Describe", &Args::One(d))?.body().deserialize()?;
            for (k, ty, doc, default) in keys {
                println!("{k}  ({ty}, default {})\n    {doc}", show(&default));
            }
        }
        (Some("get"), Some(d), Some(k), None) => {
            let v: OwnedValue = call("Get", &Args::Two(d, k))?.body().deserialize()?;
            println!("{}", show(&v));
        }
        (Some("set"), Some(d), Some(k), Some(text)) if args.len() == 4 => {
            let keys: Described = call("Describe", &Args::One(d))?.body().deserialize()?;
            let ty = keys
                .iter()
                .find(|x| x.0 == k)
                .map(|x| x.1.clone())
                .ok_or_else(|| Problem::Failed(format!("no key {k:?} in {d}")))?;
            let v = Value::parse(&ty, text).map_err(Problem::Failed)?;
            call(
                "Set",
                &Args::Set(d, k, zbus::zvariant::Value::from(v.to_variant())),
            )?;
        }
        (Some("reset"), Some(d), Some(k), None) => {
            call("Reset", &Args::Two(d, k))?;
        }
        (Some("watch"), domain, None, None) => {
            let mut rule = MatchRule::builder()
                .msg_type(message::Type::Signal)
                .sender(NAME)?
                .interface(INTERFACE)?
                .member("Changed")?;
            if let Some(d) = domain {
                rule = rule.arg(0, d)?;
            }
            for msg in MessageIterator::for_match_rule(rule.build(), &conn, None)? {
                let msg = msg?;
                let (d, k, v): (String, String, OwnedValue) = msg.body().deserialize()?;
                println!("{d}.{k} = {}", show(&v));
            }
        }
        _ => return Err(Problem::Usage),
    }
    Ok(())
}

fn show(v: &OwnedValue) -> String {
    Value::from_variant(v).map_or_else(|| format!("{v:?}"), |v| v.to_string())
}
