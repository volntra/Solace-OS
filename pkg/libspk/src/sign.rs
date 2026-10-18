//! ed25519 signing keys, detached signatures, and the trusted keyring.
//!
//! Security-critical. Formats: docs/formats/package.md ("Signature", "Keys").
//!
//! What gets signed is never the raw file but
//! `domain || sha256(file)`, where the domain names the kind of object.
//! A signature made for a package therefore never verifies as an index
//! signature, and the reverse.

use std::fmt;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};

use crate::error::{Error, IoContext, Result};
use crate::hash;

const SECKEY_HEADER: &str = "spk-seckey 1";
const PUBKEY_HEADER: &str = "spk-pubkey 1";
const SIG_HEADER: &str = "spk-signature 1";

/// Where trusted public keys live inside a root.
pub const SYSTEM_KEY_DIR: &str = "usr/share/spk/keys";
pub const ADMIN_KEY_DIR: &str = "etc/spk/keys";

/// What a signature is for. Part of the signed message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Package,
    Index,
}

impl Purpose {
    fn domain(self) -> &'static [u8] {
        match self {
            Purpose::Package => b"spk-package-v1\0",
            Purpose::Index => b"spk-index-v1\0",
        }
    }
}

fn message(purpose: Purpose, digest: &[u8; 32]) -> Vec<u8> {
    let mut m = purpose.domain().to_vec();
    m.extend_from_slice(digest);
    m
}

/// First 16 hex chars of sha256(public key bytes).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KeyId(String);

impl KeyId {
    fn of(vk: &VerifyingKey) -> Self {
        KeyId(hash::sha256_hex(vk.as_bytes())[..16].to_string())
    }
}

impl fmt::Display for KeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Parses `<header>\n<base64>\n`, returning exactly `N` decoded bytes.
fn parse_keyfile<const N: usize>(text: &str, header: &str, what: &str) -> Result<[u8; N]> {
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    if lines.next() != Some(header) {
        return Err(Error::Signature(format!(
            "{what}: missing `{header}` header"
        )));
    }
    let body = lines
        .next()
        .ok_or_else(|| Error::Signature(format!("{what}: missing key")))?;
    if lines.next().is_some() {
        return Err(Error::Signature(format!("{what}: trailing data")));
    }
    let bytes = B64
        .decode(body)
        .map_err(|e| Error::Signature(format!("{what}: {e}")))?;
    bytes
        .try_into()
        .map_err(|_| Error::Signature(format!("{what}: wrong key length")))
}

pub struct SecretKey {
    key: SigningKey,
}

impl fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretKey({})", self.key_id())
    }
}

impl SecretKey {
    /// # Errors
    /// Fails if the OS random source fails.
    pub fn generate() -> Result<Self> {
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed)
            .map_err(|e| Error::Signature(format!("no randomness for key generation: {e}")))?;
        Ok(SecretKey {
            key: SigningKey::from_bytes(&seed),
        })
    }

    pub fn public(&self) -> PublicKey {
        PublicKey {
            key: self.key.verifying_key(),
        }
    }

    pub fn key_id(&self) -> KeyId {
        KeyId::of(&self.key.verifying_key())
    }

    /// Writes `<dir>/<name>.key` (mode 0600, never overwriting) and
    /// `<dir>/<name>.pub`.
    ///
    /// # Errors
    /// I/O errors, or if the secret key file already exists.
    pub fn save(&self, dir: &Path, name: &str) -> Result<(PathBuf, PathBuf)> {
        fs::create_dir_all(dir).at(dir)?;
        let sk_path = dir.join(format!("{name}.key"));
        let pk_path = dir.join(format!("{name}.pub"));
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&sk_path)
            .at(&sk_path)?;
        writeln!(f, "{SECKEY_HEADER}\n{}", B64.encode(self.key.to_bytes())).at(&sk_path)?;
        f.sync_all().at(&sk_path)?;
        fs::write(&pk_path, self.public().to_text()).at(&pk_path)?;
        Ok((sk_path, pk_path))
    }

    /// Loads a secret key, refusing files readable by group or others.
    ///
    /// # Errors
    /// I/O errors, bad permissions, or a malformed file.
    pub fn load(path: &Path) -> Result<Self> {
        let meta = fs::metadata(path).at(path)?;
        if meta.permissions().mode() & 0o077 != 0 {
            return Err(Error::Signature(format!(
                "{} is readable by other users; run chmod 600 on it",
                path.display()
            )));
        }
        let text = fs::read_to_string(path).at(path)?;
        let seed = parse_keyfile::<32>(&text, SECKEY_HEADER, &path.display().to_string())?;
        Ok(SecretKey {
            key: SigningKey::from_bytes(&seed),
        })
    }

    pub fn sign(&self, purpose: Purpose, digest: &[u8; 32]) -> SignatureFile {
        SignatureFile {
            key_id: self.key_id(),
            sig: self.key.sign(&message(purpose, digest)).to_bytes(),
        }
    }

    /// Signs a file on disk and writes `<path>.sig` next to it.
    ///
    /// # Errors
    /// I/O errors.
    pub fn sign_file(&self, purpose: Purpose, path: &Path) -> Result<PathBuf> {
        let (digest, _) = hash::sha256_file(path)?;
        let sig_path = sig_path(path);
        fs::write(&sig_path, self.sign(purpose, &digest).to_text()).at(&sig_path)?;
        Ok(sig_path)
    }
}

/// `<file>.sig`
pub fn sig_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".sig");
    PathBuf::from(s)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicKey {
    key: VerifyingKey,
}

impl PublicKey {
    /// # Errors
    /// A malformed key file or an invalid curve point.
    pub fn parse(text: &str) -> Result<Self> {
        let bytes = parse_keyfile::<32>(text, PUBKEY_HEADER, "public key")?;
        let key = VerifyingKey::from_bytes(&bytes)
            .map_err(|e| Error::Signature(format!("public key: {e}")))?;
        if key.is_weak() {
            return Err(Error::Signature(
                "public key is a weak (small-order) key".into(),
            ));
        }
        Ok(PublicKey { key })
    }

    pub fn to_text(&self) -> String {
        format!("{PUBKEY_HEADER}\n{}\n", B64.encode(self.key.as_bytes()))
    }

    pub fn key_id(&self) -> KeyId {
        KeyId::of(&self.key)
    }
}

/// A parsed `.sig` file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureFile {
    pub key_id: KeyId,
    pub sig: [u8; 64],
}

impl SignatureFile {
    pub fn to_text(&self) -> String {
        format!(
            "{SIG_HEADER}\nkey {}\nsig {}\n",
            self.key_id,
            B64.encode(self.sig)
        )
    }

    /// # Errors
    /// Any deviation from the exact format.
    pub fn parse(text: &str) -> Result<Self> {
        let bad = |why: &str| Error::Signature(format!("malformed signature file: {why}"));
        let mut lines = text.lines();
        if lines.next() != Some(SIG_HEADER) {
            return Err(bad("missing header"));
        }
        let key = lines
            .next()
            .and_then(|l| l.strip_prefix("key "))
            .ok_or_else(|| bad("missing key line"))?;
        if key.len() != 16
            || !key
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(bad("bad key id"));
        }
        let sig = lines
            .next()
            .and_then(|l| l.strip_prefix("sig "))
            .ok_or_else(|| bad("missing sig line"))?;
        if lines.any(|l| !l.trim().is_empty()) {
            return Err(bad("trailing data"));
        }
        let sig: [u8; 64] = B64
            .decode(sig)
            .map_err(|_| bad("bad base64"))?
            .try_into()
            .map_err(|_| bad("wrong signature length"))?;
        Ok(SignatureFile {
            key_id: KeyId(key.to_string()),
            sig,
        })
    }
}

/// The set of public keys trusted for one root.
#[derive(Clone, Debug, Default)]
pub struct Keyring {
    keys: Vec<PublicKey>,
}

impl Keyring {
    pub fn new(keys: Vec<PublicKey>) -> Self {
        Keyring { keys }
    }

    /// Loads `*.pub` from `<root>/usr/share/spk/keys` and
    /// `<root>/etc/spk/keys`. Missing directories are fine; a malformed key
    /// file is an error (never silently ignored).
    ///
    /// # Errors
    /// I/O errors and malformed key files.
    pub fn load(root: &Path) -> Result<Self> {
        let mut keys = Vec::new();
        for dir in [SYSTEM_KEY_DIR, ADMIN_KEY_DIR] {
            let dir = root.join(dir);
            let Ok(rd) = fs::read_dir(&dir) else { continue };
            let mut paths: Vec<_> = rd
                .filter_map(std::result::Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "pub"))
                .collect();
            paths.sort();
            for p in paths {
                let text = fs::read_to_string(&p).at(&p)?;
                let k = PublicKey::parse(&text).map_err(|e| e.sig_context(p.display()))?;
                keys.push(k);
            }
        }
        Ok(Keyring { keys })
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub fn keys(&self) -> &[PublicKey] {
        &self.keys
    }

    /// Verifies `sig_text` over `digest` for `purpose`.
    ///
    /// # Errors
    /// [`Error::Signature`] if the signature is malformed, made by a key not
    /// in this keyring, or does not verify.
    pub fn verify(&self, purpose: Purpose, digest: &[u8; 32], sig_text: &str) -> Result<KeyId> {
        let sf = SignatureFile::parse(sig_text)?;
        let key = self
            .keys
            .iter()
            .find(|k| k.key_id() == sf.key_id)
            .ok_or_else(|| {
                Error::Signature(format!("signed by key {} which is not trusted", sf.key_id))
            })?;
        key.key
            .verify_strict(&message(purpose, digest), &Signature::from_bytes(&sf.sig))
            .map_err(|_| Error::Signature(format!("bad signature (key {})", sf.key_id)))?;
        Ok(sf.key_id)
    }

    /// Verifies `<path>.sig` against the file at `path`.
    ///
    /// # Errors
    /// I/O errors (including a missing `.sig`) and [`Keyring::verify`] errors.
    pub fn verify_file(&self, purpose: Purpose, path: &Path) -> Result<KeyId> {
        let sp = sig_path(path);
        let sig_text = fs::read_to_string(&sp).map_err(|e| {
            Error::Signature(format!("{}: cannot read signature: {e}", sp.display()))
        })?;
        let (digest, _) = hash::sha256_file(path)?;
        self.verify(purpose, &digest, &sig_text)
            .map_err(|e| e.sig_context(path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(s: &[u8]) -> [u8; 32] {
        hash::sha256(s)
    }

    #[test]
    fn sign_and_verify() {
        let sk = SecretKey::generate().unwrap();
        let ring = Keyring::new(vec![sk.public()]);
        let sig = sk.sign(Purpose::Package, &digest(b"pkg")).to_text();
        assert_eq!(
            ring.verify(Purpose::Package, &digest(b"pkg"), &sig)
                .unwrap(),
            sk.key_id()
        );
    }

    #[test]
    fn tampered_data_fails() {
        let sk = SecretKey::generate().unwrap();
        let ring = Keyring::new(vec![sk.public()]);
        let sig = sk.sign(Purpose::Package, &digest(b"pkg")).to_text();
        assert!(
            ring.verify(Purpose::Package, &digest(b"pkg!"), &sig)
                .is_err()
        );
    }

    #[test]
    fn domain_separation() {
        let sk = SecretKey::generate().unwrap();
        let ring = Keyring::new(vec![sk.public()]);
        let sig = sk.sign(Purpose::Package, &digest(b"x")).to_text();
        assert!(ring.verify(Purpose::Index, &digest(b"x"), &sig).is_err());
    }

    #[test]
    fn untrusted_key_fails() {
        let sk = SecretKey::generate().unwrap();
        let other = SecretKey::generate().unwrap();
        let ring = Keyring::new(vec![other.public()]);
        let sig = sk.sign(Purpose::Package, &digest(b"x")).to_text();
        let err = ring
            .verify(Purpose::Package, &digest(b"x"), &sig)
            .unwrap_err();
        assert!(err.to_string().contains("not trusted"));
        assert!(
            Keyring::default()
                .verify(Purpose::Package, &digest(b"x"), &sig)
                .is_err()
        );
    }

    #[test]
    fn key_id_spoofing_fails() {
        // A signature by an untrusted key that claims a trusted key's id.
        let trusted = SecretKey::generate().unwrap();
        let attacker = SecretKey::generate().unwrap();
        let ring = Keyring::new(vec![trusted.public()]);
        let mut sf = attacker.sign(Purpose::Package, &digest(b"x"));
        sf.key_id = trusted.key_id();
        assert!(
            ring.verify(Purpose::Package, &digest(b"x"), &sf.to_text())
                .is_err()
        );
    }

    #[test]
    fn flipped_signature_bit_fails() {
        let sk = SecretKey::generate().unwrap();
        let ring = Keyring::new(vec![sk.public()]);
        let mut sf = sk.sign(Purpose::Package, &digest(b"x"));
        sf.sig[10] ^= 1;
        assert!(
            ring.verify(Purpose::Package, &digest(b"x"), &sf.to_text())
                .is_err()
        );
    }

    #[test]
    fn malformed_signature_files() {
        let sk = SecretKey::generate().unwrap();
        let ring = Keyring::new(vec![sk.public()]);
        let good = sk.sign(Purpose::Package, &digest(b"x")).to_text();
        let cases = [
            String::new(),
            good.replace("spk-signature 1", "spk-signature 2"),
            good.replace("key ", "kee "),
            good.replace("sig ", "sig !!"),
            format!("{good}extra\n"),
            good.lines().take(2).collect::<Vec<_>>().join("\n"),
        ];
        for c in cases {
            assert!(
                ring.verify(Purpose::Package, &digest(b"x"), &c).is_err(),
                "{c:?}"
            );
        }
    }

    #[test]
    fn key_files_roundtrip_and_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let sk = SecretKey::generate().unwrap();
        let (skp, pkp) = sk.save(dir.path(), "dev").unwrap();
        assert_eq!(
            fs::metadata(&skp).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(sk.save(dir.path(), "dev").is_err(), "must not overwrite");

        let loaded = SecretKey::load(&skp).unwrap();
        assert_eq!(loaded.key_id(), sk.key_id());
        let pk = PublicKey::parse(&fs::read_to_string(&pkp).unwrap()).unwrap();
        assert_eq!(pk.key_id(), sk.key_id());

        fs::set_permissions(&skp, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            SecretKey::load(&skp)
                .unwrap_err()
                .to_string()
                .contains("chmod 600")
        );
    }

    #[test]
    fn keyring_loads_from_root_and_rejects_garbage() {
        let root = tempfile::tempdir().unwrap();
        let sk = SecretKey::generate().unwrap();
        let d = root.path().join(ADMIN_KEY_DIR);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("dev.pub"), sk.public().to_text()).unwrap();
        fs::write(d.join("README"), "ignored").unwrap();
        let ring = Keyring::load(root.path()).unwrap();
        assert_eq!(ring.keys().len(), 1);

        fs::write(d.join("bad.pub"), "spk-pubkey 1\nAAAA\n").unwrap();
        assert!(Keyring::load(root.path()).is_err());
    }

    #[test]
    fn sign_file_and_verify_file() {
        let dir = tempfile::tempdir().unwrap();
        let sk = SecretKey::generate().unwrap();
        let ring = Keyring::new(vec![sk.public()]);
        let f = dir.path().join("a.solpkg");
        fs::write(&f, b"payload").unwrap();
        sk.sign_file(Purpose::Package, &f).unwrap();
        ring.verify_file(Purpose::Package, &f).unwrap();

        fs::write(&f, b"payload2").unwrap();
        assert!(ring.verify_file(Purpose::Package, &f).is_err());

        fs::remove_file(sig_path(&f)).unwrap();
        assert!(ring.verify_file(Purpose::Package, &f).is_err());
    }
}
