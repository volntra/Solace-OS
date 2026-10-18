//! Versions and dependency specs.
//!
//! Upstream versions compare segment by segment, in the style of rpm's
//! `rpmvercmp`: digit runs compare numerically, letter runs lexically, a
//! numeric segment is newer than an alphabetic one, and `~` sorts before
//! everything (so `1.0~rc1 < 1.0`). A package version is an upstream version
//! plus our integer release.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{Error, Result};

/// An upstream version string, e.g. `5.8.1` or `1.0~rc2`.
#[derive(Clone)]
pub struct Version(String);

impl Version {
    /// # Errors
    /// Rejects empty versions and characters outside `[A-Za-z0-9.+~_]`.
    pub fn parse(s: &str) -> Result<Self> {
        if s.is_empty() || s.len() > 128 {
            return Err(Error::invalid(format!("invalid version {s:?}")));
        }
        if !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'+' | b'~' | b'_'))
        {
            return Err(Error::invalid(format!(
                "invalid version {s:?}: only letters, digits, and . + ~ _ are allowed"
            )));
        }
        if !s.bytes().any(|b| b.is_ascii_alphanumeric()) {
            return Err(Error::invalid(format!("invalid version {s:?}")));
        }
        Ok(Version(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One comparable piece of a version.
#[derive(Debug, PartialEq, Eq, Hash)]
enum Segment<'a> {
    Tilde,
    Num(&'a str), // leading zeros stripped
    Alpha(&'a str),
}

fn segments(s: &str) -> impl Iterator<Item = Segment<'_>> {
    let b = s.as_bytes();
    let mut i = 0;
    std::iter::from_fn(move || {
        while i < b.len() && !b[i].is_ascii_alphanumeric() && b[i] != b'~' {
            i += 1;
        }
        if i >= b.len() {
            return None;
        }
        if b[i] == b'~' {
            i += 1;
            return Some(Segment::Tilde);
        }
        let start = i;
        if b[i].is_ascii_digit() {
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            let digits = s[start..i].trim_start_matches('0');
            Some(Segment::Num(digits))
        } else {
            while i < b.len() && b[i].is_ascii_alphabetic() {
                i += 1;
            }
            Some(Segment::Alpha(&s[start..i]))
        }
    })
}

/// Compares two version strings (see module docs).
pub fn vercmp(a: &str, b: &str) -> Ordering {
    let mut sa = segments(a);
    let mut sb = segments(b);
    loop {
        match (sa.next(), sb.next()) {
            (None, None) => return Ordering::Equal,
            // A trailing tilde makes the longer version older.
            (Some(Segment::Tilde), None) => return Ordering::Less,
            (None, Some(Segment::Tilde)) => return Ordering::Greater,
            (Some(_), None) => return Ordering::Greater,
            (None, Some(_)) => return Ordering::Less,
            (Some(x), Some(y)) => {
                let ord = match (x, y) {
                    (Segment::Tilde, Segment::Tilde) => Ordering::Equal,
                    (Segment::Tilde, _) => Ordering::Less,
                    (_, Segment::Tilde) => Ordering::Greater,
                    (Segment::Num(x), Segment::Num(y)) => {
                        x.len().cmp(&y.len()).then_with(|| x.cmp(y))
                    }
                    (Segment::Alpha(x), Segment::Alpha(y)) => x.cmp(y),
                    (Segment::Num(_), Segment::Alpha(_)) => Ordering::Greater,
                    (Segment::Alpha(_), Segment::Num(_)) => Ordering::Less,
                };
                if ord != Ordering::Equal {
                    return ord;
                }
            }
        }
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        vercmp(&self.0, &other.0)
    }
}
impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for Version {}
impl Hash for Version {
    // Must agree with `Eq`: hash the normalised segments, not the string.
    fn hash<H: Hasher>(&self, state: &mut H) {
        for s in segments(&self.0) {
            s.hash(state);
        }
    }
}
impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl fmt::Debug for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Version({})", self.0)
    }
}
impl FromStr for Version {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        Version::parse(s)
    }
}
impl Serialize for Version {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}
impl<'de> Deserialize<'de> for Version {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Version::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// Upstream version + our release: what a package actually is.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PkgVersion {
    pub version: Version,
    pub release: u32,
}

impl PkgVersion {
    pub fn new(version: Version, release: u32) -> Self {
        PkgVersion { version, release }
    }
}

impl fmt::Display for PkgVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.version, self.release)
    }
}

/// Validates a package name: `[a-z0-9][a-z0-9+._-]*`, at most 64 bytes.
///
/// # Errors
/// Returns [`Error::Invalid`] describing the problem.
pub fn validate_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && name.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'+' | b'.' | b'_' | b'-')
        });
    if ok {
        Ok(())
    } else {
        Err(Error::invalid(format!(
            "invalid package name {name:?}: use lowercase letters, digits, and + . _ -"
        )))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Op {
    Lt,
    Le,
    Eq,
    Ge,
    Gt,
}

impl Op {
    fn as_str(self) -> &'static str {
        match self {
            Op::Lt => "<",
            Op::Le => "<=",
            Op::Eq => "=",
            Op::Ge => ">=",
            Op::Gt => ">",
        }
    }

    fn holds(self, ord: Ordering) -> bool {
        match self {
            Op::Lt => ord == Ordering::Less,
            Op::Le => ord != Ordering::Greater,
            Op::Eq => ord == Ordering::Equal,
            Op::Ge => ord != Ordering::Less,
            Op::Gt => ord == Ordering::Greater,
        }
    }
}

/// A version constraint: `>= 1.2`, `= 1.2-3`, ...
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Constraint {
    pub op: Op,
    pub version: Version,
    /// When set, the release takes part in the comparison.
    pub release: Option<u32>,
}

impl Constraint {
    pub fn matches(&self, v: &PkgVersion) -> bool {
        let ord = match self.release {
            Some(r) => v.version.cmp(&self.version).then(v.release.cmp(&r)),
            None => v.version.cmp(&self.version),
        };
        self.op.holds(ord)
    }
}

impl fmt::Display for Constraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.op.as_str(), self.version)?;
        if let Some(r) = self.release {
            write!(f, "-{r}")?;
        }
        Ok(())
    }
}

/// A dependency: a name and an optional constraint, e.g. `zlib>=1.3`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DepSpec {
    pub name: String,
    pub constraint: Option<Constraint>,
}

impl DepSpec {
    /// # Errors
    /// Returns [`Error::Invalid`] for a malformed spec.
    pub fn parse(s: &str) -> Result<Self> {
        let s = s.trim();
        let Some(idx) = s.find(['<', '>', '=']) else {
            validate_name(s)?;
            return Ok(DepSpec {
                name: s.to_string(),
                constraint: None,
            });
        };
        let name = s[..idx].trim();
        validate_name(name)?;
        let rest = &s[idx..];
        let (op, rest) = [
            ("<=", Op::Le),
            (">=", Op::Ge),
            ("<", Op::Lt),
            (">", Op::Gt),
            ("=", Op::Eq),
        ]
        .iter()
        .find_map(|(p, op)| rest.strip_prefix(p).map(|r| (*op, r.trim())))
        .ok_or_else(|| Error::invalid(format!("invalid dependency {s:?}")))?;
        let (ver, release) = match rest.rsplit_once('-') {
            Some((v, r)) => {
                let r = r
                    .parse::<u32>()
                    .map_err(|_| Error::invalid(format!("invalid release in dependency {s:?}")))?;
                (v, Some(r))
            }
            None => (rest, None),
        };
        Ok(DepSpec {
            name: name.to_string(),
            constraint: Some(Constraint {
                op,
                version: Version::parse(ver)?,
                release,
            }),
        })
    }

    pub fn matches(&self, name: &str, v: &PkgVersion) -> bool {
        self.name == name && self.constraint.as_ref().is_none_or(|c| c.matches(v))
    }
}

impl fmt::Display for DepSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)?;
        if let Some(c) = &self.constraint {
            write!(f, "{c}")?;
        }
        Ok(())
    }
}

impl FromStr for DepSpec {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        DepSpec::parse(s)
    }
}
impl Serialize for DepSpec {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}
impl<'de> Deserialize<'de> for DepSpec {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        DepSpec::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn ordering() {
        let ordered = [
            "0.9",
            "1.0~alpha",
            "1.0~rc1",
            "1.0~rc2",
            "1.0",
            "1.0a",
            "1.0.1",
            "1.1",
            "1.2",
            "1.10",
            "2",
            "2024.01",
            "2024.1.1",
        ];
        for w in ordered.windows(2) {
            assert!(v(w[0]) < v(w[1]), "{} < {}", w[0], w[1]);
            assert!(v(w[1]) > v(w[0]), "{} > {}", w[1], w[0]);
        }
    }

    #[test]
    fn numeric_beats_alpha_and_leading_zeros_ignored() {
        assert!(v("1.0.1") > v("1.0.a"));
        assert_eq!(v("1.01"), v("1.1"));
        assert_eq!(v("1_0"), v("1.0"));
        let mut h1 = std::collections::hash_map::DefaultHasher::new();
        let mut h2 = std::collections::hash_map::DefaultHasher::new();
        v("1.01").hash(&mut h1);
        v("1.1").hash(&mut h2);
        assert_eq!(h1.finish(), h2.finish());
    }

    #[test]
    fn invalid_versions() {
        for bad in ["", "1.0-1", "1 0", "...", "1/2", "é"] {
            assert!(Version::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn names() {
        for good in ["zlib", "libstdc++", "python3.13", "x_y-z", "0ad"] {
            validate_name(good).unwrap();
        }
        for bad in ["", "Zlib", "-x", ".x", "a b", "a/b", &"a".repeat(65)] {
            assert!(validate_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn depspec_parse_and_display() {
        for (s, disp) in [
            ("zlib", "zlib"),
            ("zlib>=1.3", "zlib>=1.3"),
            ("zlib >= 1.3", "zlib>=1.3"),
            ("xz=5.8.1-2", "xz=5.8.1-2"),
            ("a<2", "a<2"),
            ("a<=2", "a<=2"),
            ("a>2", "a>2"),
        ] {
            assert_eq!(DepSpec::parse(s).unwrap().to_string(), disp);
        }
        for bad in ["", "zlib>=", "zlib=>1", "zlib>=1.0-x", "Zlib", "z!=1"] {
            assert!(DepSpec::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn depspec_matching() {
        let pv = |s: &str, r| PkgVersion::new(v(s), r);
        let d = DepSpec::parse("xz>=5.8").unwrap();
        assert!(d.matches("xz", &pv("5.8", 1)));
        assert!(d.matches("xz", &pv("5.8.1", 1)));
        assert!(!d.matches("xz", &pv("5.7.9", 9)));
        assert!(!d.matches("zstd", &pv("5.8", 1)));

        let exact = DepSpec::parse("xz=5.8.1").unwrap();
        assert!(exact.matches("xz", &pv("5.8.1", 1)));
        assert!(exact.matches("xz", &pv("5.8.1", 7)));

        let pinned = DepSpec::parse("xz=5.8.1-2").unwrap();
        assert!(!pinned.matches("xz", &pv("5.8.1", 1)));
        assert!(pinned.matches("xz", &pv("5.8.1", 2)));

        let lt = DepSpec::parse("xz<5.8.1-2").unwrap();
        assert!(lt.matches("xz", &pv("5.8.1", 1)));
        assert!(!lt.matches("xz", &pv("5.8.1", 2)));

        assert!(DepSpec::parse("a").unwrap().matches("a", &pv("0", 1)));
    }

    #[test]
    fn pkgversion_orders_release_after_version() {
        let a = PkgVersion::new(v("1.0"), 9);
        let b = PkgVersion::new(v("1.0.1"), 1);
        let c = PkgVersion::new(v("1.0.1"), 2);
        assert!(a < b && b < c);
        assert_eq!(c.to_string(), "1.0.1-2");
    }
}
