//! The D-Bus side: `org.fern.Settings1` at `/org/fern/Settings`, owned as
//! `org.fern.Settings` on the session bus (the user's domains) or the
//! system bus (the system's). docs/protocols/settings.md.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

use zbus::message::Header;
use zbus::names::BusName;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedValue, Value as Variant};
use zbus::{Connection, fdo, interface};

use crate::Value;
use crate::schema::Scope;
use crate::store::{self, Store};

pub const NAME: &str = "org.fern.Settings";
pub const PATH: &str = "/org/fern/Settings";
pub const INTERFACE: &str = "org.fern.Settings1";

/// Which bus to serve on.
#[derive(Clone, Debug)]
pub enum Bus {
    Session,
    System,
    Address(String),
}

#[derive(Debug)]
pub struct Settings {
    store: Mutex<Store>,
    /// Run after the boot domain changes (`spk boot-menu` regenerates
    /// grub.cfg); a failure undoes the change.
    boot_hook: Option<Vec<String>>,
}

impl Settings {
    pub fn new(store: Store, boot_hook: Option<Vec<String>>) -> Settings {
        Settings {
            store: Mutex::new(store),
            boot_hook,
        }
    }

    fn scope(&self) -> Scope {
        self.lock().scope()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Store> {
        // A panic while holding the lock leaves the store consistent (every
        // change is written before memory is updated).
        self.store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// System domains can be written by other programs (spk sets the boot
    /// default): read the file again before answering. Returns the values
    /// that changed, to announce.
    fn refresh(store: &mut Store, domain: &str) -> fdo::Result<Vec<(String, Value)>> {
        if store.scope() != Scope::System {
            return Ok(Vec::new());
        }
        let (changed, warnings) = store.reload(domain).map_err(to_fdo)?;
        for w in warnings {
            eprintln!("fern-settingsd: {w}");
        }
        changed
            .into_iter()
            .map(|k| Ok((k.to_owned(), store.get(domain, k).map_err(to_fdo)?)))
            .collect()
    }

    /// Only root (or the daemon's own user, for tests) may change system
    /// settings. Polkit takes over when it is in the image.
    async fn authorize(&self, conn: &Connection, hdr: &Header<'_>) -> fdo::Result<()> {
        if self.scope() == Scope::User {
            return Ok(());
        }
        let sender = hdr
            .sender()
            .ok_or_else(|| fdo::Error::AccessDenied("no sender".into()))?;
        let uid = fdo::DBusProxy::new(conn)
            .await?
            .get_connection_unix_user(BusName::Unique(sender.to_owned()))
            .await?;
        if uid == 0 || uid == rustix::process::geteuid().as_raw() {
            Ok(())
        } else {
            Err(fdo::Error::AccessDenied(
                "only an administrator can change system settings".into(),
            ))
        }
    }

    fn hook(&self, domain: &str) -> Option<impl FnMut(&Path) -> Result<(), String> + use<'_>> {
        let cmd = self.boot_hook.as_ref().filter(|_| domain == "boot")?;
        Some(move |_: &Path| {
            let out = Command::new(&cmd[0])
                .args(&cmd[1..])
                .output()
                .map_err(|e| format!("{}: {e}", cmd[0]))?;
            if out.status.success() {
                Ok(())
            } else {
                let err = String::from_utf8_lossy(&out.stderr);
                let last = err
                    .lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("");
                Err(format!("{} failed ({}): {last}", cmd.join(" "), out.status))
            }
        })
    }

    /// Set (`Some`) or reset (`None`), then announce what changed.
    async fn change(
        &self,
        emitter: &SignalEmitter<'_>,
        domain: &str,
        key: &str,
        value: Option<Value>,
    ) -> fdo::Result<()> {
        let mut announce = {
            let mut store = self.lock();
            let mut announce = Settings::refresh(&mut store, domain)?;
            let mut hook = self.hook(domain);
            let apply = hook.as_mut().map(|h| h as store::Apply<'_>);
            let changed = match value {
                Some(v) => store.set(domain, key, v, apply),
                None => store.reset(domain, key, apply),
            }
            .map_err(to_fdo)?;
            if changed {
                announce.push((key.to_owned(), store.get(domain, key).map_err(to_fdo)?));
            }
            announce
        };
        announce.dedup_by(|a, b| a.0 == b.0);
        for (k, v) in announce {
            Settings::changed(emitter, domain, &k, &Variant::from(v.to_variant())).await?;
        }
        Ok(())
    }

    async fn announce(
        emitter: &SignalEmitter<'_>,
        domain: &str,
        changed: Vec<(String, Value)>,
    ) -> fdo::Result<()> {
        for (k, v) in changed {
            Settings::changed(emitter, domain, &k, &Variant::from(v.to_variant())).await?;
        }
        Ok(())
    }
}

// By value, so it reads as `.map_err(to_fdo)`.
#[allow(clippy::needless_pass_by_value)]
fn to_fdo(e: store::Error) -> fdo::Error {
    match e {
        store::Error::UnknownDomain(_)
        | store::Error::UnknownKey(..)
        | store::Error::Invalid(_) => fdo::Error::InvalidArgs(e.to_string()),
        store::Error::Io(_) | store::Error::Apply(_) => fdo::Error::Failed(e.to_string()),
    }
}

#[interface(name = "org.fern.Settings1")]
impl Settings {
    /// The domains this daemon owns.
    fn domains(&self) -> Vec<String> {
        self.lock().domains().map(|d| d.name.to_owned()).collect()
    }

    /// Every key of a domain: (name, type, description, default). Types are
    /// listed in docs/protocols/settings.md.
    fn describe(&self, domain: &str) -> fdo::Result<Vec<(String, String, String, OwnedValue)>> {
        let store = self.lock();
        let d = store
            .domains()
            .find(|d| d.name == domain)
            .ok_or_else(|| to_fdo(store::Error::UnknownDomain(domain.into())))?;
        Ok(d.keys
            .iter()
            .map(|k| {
                (
                    k.name.to_owned(),
                    k.kind.describe(),
                    k.doc.to_owned(),
                    k.default.value().to_variant(),
                )
            })
            .collect())
    }

    async fn get(
        &self,
        domain: &str,
        key: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<OwnedValue> {
        let (v, changed) = {
            let mut store = self.lock();
            let changed = Settings::refresh(&mut store, domain)?;
            (store.get(domain, key).map_err(to_fdo)?, changed)
        };
        Settings::announce(&emitter, domain, changed).await?;
        Ok(v.to_variant())
    }

    async fn get_all(
        &self,
        domain: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<HashMap<String, OwnedValue>> {
        let (all, changed) = {
            let mut store = self.lock();
            let changed = Settings::refresh(&mut store, domain)?;
            (store.all(domain).map_err(to_fdo)?, changed)
        };
        Settings::announce(&emitter, domain, changed).await?;
        Ok(all
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_variant()))
            .collect())
    }

    async fn set(
        &self,
        domain: &str,
        key: &str,
        value: OwnedValue,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        self.authorize(conn, &hdr).await?;
        let v = Value::from_variant(&value).ok_or_else(|| {
            fdo::Error::InvalidArgs(format!(
                "{domain}.{key}: a value must be a bool, an integer, a string or a list of strings, not {}",
                value.value_signature()
            ))
        })?;
        self.change(&emitter, domain, key, Some(v)).await
    }

    async fn reset(
        &self,
        domain: &str,
        key: &str,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        self.authorize(conn, &hdr).await?;
        self.change(&emitter, domain, key, None).await
    }

    /// A value changed (also sent when another program changed a system
    /// file, the next time the domain is read).
    #[zbus(signal)]
    async fn changed(
        emitter: &SignalEmitter<'_>,
        domain: &str,
        key: &str,
        value: &Variant<'_>,
    ) -> zbus::Result<()>;
}

/// Connects to `bus`.
///
/// # Errors
/// When the bus cannot be reached.
pub async fn connect(bus: &Bus) -> zbus::Result<Connection> {
    match bus {
        Bus::Session => Connection::session().await,
        Bus::System => Connection::system().await,
        Bus::Address(a) => {
            zbus::connection::Builder::address(a.as_str())?
                .build()
                .await
        }
    }
}

/// Serves `settings` on `conn`, then takes the well-known name (so no
/// client sees the name before the object exists).
///
/// # Errors
/// When the name is taken.
pub async fn serve(conn: &Connection, settings: Settings) -> zbus::Result<()> {
    conn.object_server().at(PATH, settings).await?;
    conn.request_name_with_flags(NAME, zbus::fdo::RequestNameFlags::DoNotQueue.into())
        .await
        .map(|_| ())
}
