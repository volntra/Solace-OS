//! The settings schema: every domain, its keys, their types and defaults
//! (docs/protocols/settings.md). A key is in the schema only once a
//! component reads it.

use crate::Value;

/// Which daemon owns a domain: the user's (session bus, files in
/// `~/.config/fern/`) or the system's (system bus, files in `/etc/fern/`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    User,
    System,
}

/// What a text value may contain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Text {
    /// Printable text, at most 256 bytes.
    Any,
    /// `[A-Za-z0-9._-]*`, at most 64 bytes (snapshot and app ids).
    Id,
    /// An absolute path, at most 4096 bytes.
    Path,
    /// An accent: a preset name, `wallpaper`, or `#rrggbb`.
    Accent,
}

/// A key's type, with the values it accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Bool,
    Int {
        min: i64,
        max: i64,
    },
    Choice(&'static [&'static str]),
    /// `optional`: the empty string means "not set" and is not written to
    /// the file (bootgen reads a missing key as its own default).
    Text {
        rule: Text,
        optional: bool,
    },
    List {
        item: Text,
        max: usize,
    },
}

/// A key's default, in `static` form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Default {
    Bool(bool),
    Int(i64),
    Text(&'static str),
    List(&'static [&'static str]),
}

impl Default {
    pub fn value(self) -> Value {
        match self {
            Default::Bool(b) => Value::Bool(b),
            Default::Int(i) => Value::Int(i),
            Default::Text(s) => Value::Text(s.to_owned()),
            Default::List(l) => Value::List(l.iter().map(|s| (*s).to_owned()).collect()),
        }
    }
}

#[derive(Debug)]
pub struct Key {
    pub name: &'static str,
    pub kind: Kind,
    pub default: Default,
    pub doc: &'static str,
}

#[derive(Debug)]
pub struct Domain {
    pub name: &'static str,
    pub scope: Scope,
    pub doc: &'static str,
    pub keys: &'static [Key],
}

impl Domain {
    pub fn key(&self, name: &str) -> Option<&'static Key> {
        self.keys.iter().find(|k| k.name == name)
    }
}

/// Accent presets (the colours themselves live in the design system).
pub const ACCENTS: &[&str] = &[
    "fern", "blue", "purple", "pink", "orange", "amber", "graphite",
];

const fn key(name: &'static str, kind: Kind, default: Default, doc: &'static str) -> Key {
    Key {
        name,
        kind,
        default,
        doc,
    }
}

const BOOL: Kind = Kind::Bool;

pub static DOMAINS: &[Domain] = &[
    Domain {
        name: "appearance",
        scope: Scope::User,
        doc: "Light and dark, the accent colour, motion and transparency",
        keys: &[
            key(
                "theme",
                Kind::Choice(&["light", "dark", "auto"]),
                Default::Text("light"),
                "Light, dark, or dark from 19:00 to 7:00",
            ),
            key(
                "accent",
                Kind::Text {
                    rule: Text::Accent,
                    optional: false,
                },
                Default::Text("fern"),
                "Accent colour: a preset, wallpaper, or #rrggbb",
            ),
            key(
                "reduce_motion",
                BOOL,
                Default::Bool(false),
                "Fade instead of moving and zooming",
            ),
            key(
                "reduce_transparency",
                BOOL,
                Default::Bool(false),
                "Solid surfaces instead of translucent materials",
            ),
            key(
                "text_size",
                Kind::Choice(&["small", "default", "large", "larger"]),
                Default::Text("default"),
                "Text size across the desktop",
            ),
        ],
    },
    Domain {
        name: "wallpaper",
        scope: Scope::User,
        doc: "The desktop picture, with an optional dark-mode partner",
        keys: &[
            key(
                "light",
                Kind::Text {
                    rule: Text::Path,
                    optional: false,
                },
                Default::Text("/usr/share/fern/wallpapers/grove.png"),
                "Picture in light mode",
            ),
            key(
                "dark",
                Kind::Text {
                    rule: Text::Path,
                    optional: true,
                },
                Default::Text("/usr/share/fern/wallpapers/grove-night.png"),
                "Picture in dark mode (empty: the light one)",
            ),
        ],
    },
    Domain {
        name: "dock",
        scope: Scope::User,
        doc: "The dock",
        keys: &[
            key(
                "position",
                Kind::Choice(&["bottom", "left", "right"]),
                Default::Text("bottom"),
                "Screen edge the dock sits on",
            ),
            key(
                "icon_size",
                Kind::Int { min: 32, max: 96 },
                Default::Int(52),
                "Icon size in pixels",
            ),
            key(
                "magnification",
                BOOL,
                Default::Bool(true),
                "Icons grow under the pointer",
            ),
            key(
                "magnified_size",
                Kind::Int { min: 48, max: 128 },
                Default::Int(76),
                "Largest magnified icon size in pixels",
            ),
            key(
                "autohide",
                BOOL,
                Default::Bool(false),
                "Hide the dock until the pointer reaches its edge",
            ),
            key(
                "pinned",
                Kind::List {
                    item: Text::Id,
                    max: 64,
                },
                Default::List(&["org.fern.Files", "org.fern.Terminal", "org.fern.Settings"]),
                "Apps kept in the dock, by app id",
            ),
            key(
                "show_recent",
                BOOL,
                Default::Bool(true),
                "Show recently used apps that are not pinned",
            ),
        ],
    },
    Domain {
        name: "menubar",
        scope: Scope::User,
        doc: "The menu bar and its clock",
        keys: &[
            key(
                "translucent",
                BOOL,
                Default::Bool(true),
                "Let the wallpaper show through",
            ),
            key("clock_24h", BOOL, Default::Bool(false), "24-hour clock"),
            key(
                "show_date",
                BOOL,
                Default::Bool(true),
                "Show the date next to the time",
            ),
            key("show_seconds", BOOL, Default::Bool(false), "Show seconds"),
        ],
    },
    Domain {
        name: "notifications",
        scope: Scope::User,
        doc: "Notification banners and Do Not Disturb",
        keys: &[
            key(
                "dnd",
                BOOL,
                Default::Bool(false),
                "Do Not Disturb: no banners, history only",
            ),
            key(
                "banner_seconds",
                Kind::Int { min: 2, max: 30 },
                Default::Int(5),
                "How long a banner stays",
            ),
        ],
    },
    // Read by fern-bootgen (through spk) as /etc/fern/boot.toml; the keys
    // mirror bootgen::Settings exactly, which refuses unknown ones.
    Domain {
        name: "boot",
        scope: Scope::System,
        doc: "The boot menu (fern-bootgen)",
        keys: &[
            key(
                "menu",
                Kind::Choice(&["always", "hidden", "never"]),
                Default::Text("hidden"),
                "Always show the menu, show it only while Shift is held, or never",
            ),
            key(
                "timeout",
                Kind::Int { min: 0, max: 300 },
                Default::Int(5),
                "Seconds the menu waits",
            ),
            key(
                "theme",
                BOOL,
                Default::Bool(true),
                "Fern theme (or plain text)",
            ),
            key(
                "verbose",
                BOOL,
                Default::Bool(false),
                "Show startup messages instead of the splash",
            ),
            key(
                "default",
                Kind::Text {
                    rule: Text::Id,
                    optional: true,
                },
                Default::Text(""),
                "System to start by default (empty: the newest)",
            ),
            key(
                "keep",
                Kind::Int { min: 1, max: 64 },
                Default::Int(3),
                "How many systems stay bootable",
            ),
            key(
                "other_os",
                BOOL,
                Default::Bool(true),
                "List other operating systems",
            ),
            key(
                "console",
                Kind::List {
                    item: Text::Any,
                    max: 8,
                },
                Default::List(&[]),
                "Kernel consoles (console=), the last one is /dev/console",
            ),
        ],
    },
];

pub fn domain(name: &str) -> Option<&'static Domain> {
    DOMAINS.iter().find(|d| d.name == name)
}

pub fn domains(scope: Scope) -> impl Iterator<Item = &'static Domain> {
    DOMAINS.iter().filter(move |d| d.scope == scope)
}

impl Text {
    fn check(self, s: &str) -> Result<(), String> {
        let ok = match self {
            Text::Any => s.len() <= 256 && !s.chars().any(char::is_control),
            Text::Id => {
                s.len() <= 64
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            }
            Text::Path => s.len() <= 4096 && s.starts_with('/') && !s.contains('\0'),
            Text::Accent => {
                ACCENTS.contains(&s)
                    || s == "wallpaper"
                    || (s.len() == 7
                        && s.starts_with('#')
                        && s[1..].bytes().all(|b| b.is_ascii_hexdigit()))
            }
        };
        if ok {
            Ok(())
        } else {
            Err(format!("{s:?} is not {}", self.describe()))
        }
    }

    fn describe(self) -> &'static str {
        match self {
            Text::Any => "text",
            Text::Id => "an id ([A-Za-z0-9._-])",
            Text::Path => "an absolute path",
            Text::Accent => "an accent (a preset, wallpaper or #rrggbb)",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Text::Any => "text",
            Text::Id => "id",
            Text::Path => "path",
            Text::Accent => "accent",
        }
    }
}

impl Kind {
    /// Checks `v` against this type.
    ///
    /// # Errors
    /// A message saying what was expected.
    pub fn check(self, v: &Value) -> Result<(), String> {
        match (self, v) {
            (Kind::Bool, Value::Bool(_)) => Ok(()),
            (Kind::Int { min, max }, Value::Int(i)) => {
                if (min..=max).contains(i) {
                    Ok(())
                } else {
                    Err(format!("{i} is outside {min}..{max}"))
                }
            }
            (Kind::Choice(c), Value::Text(s)) => {
                if c.contains(&s.as_str()) {
                    Ok(())
                } else {
                    Err(format!("{s:?} is not one of {}", c.join(", ")))
                }
            }
            (Kind::Text { rule, optional }, Value::Text(s)) => {
                if optional && s.is_empty() {
                    Ok(())
                } else {
                    rule.check(s)
                }
            }
            (Kind::List { item, max }, Value::List(l)) => {
                if l.len() > max {
                    return Err(format!("more than {max} items"));
                }
                l.iter().try_for_each(|s| item.check(s))
            }
            _ => Err(format!(
                "expected {}, got {}",
                self.describe(),
                v.type_name()
            )),
        }
    }

    /// The type as `Describe` reports it: `bool`, `int MIN..MAX`,
    /// `choice A|B|C`, `text`, `id`, `path`, `accent` (each with `?` when
    /// optional), or `list ITEM`.
    pub fn describe(self) -> String {
        match self {
            Kind::Bool => "bool".into(),
            Kind::Int { min, max } => format!("int {min}..{max}"),
            Kind::Choice(c) => format!("choice {}", c.join("|")),
            Kind::Text { rule, optional } => {
                format!("{}{}", rule.name(), if optional { "?" } else { "" })
            }
            Kind::List { item, .. } => format!("list {}", item.name()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_satisfy_their_own_types() {
        for d in DOMAINS {
            for k in d.keys {
                k.kind
                    .check(&k.default.value())
                    .unwrap_or_else(|e| panic!("{}.{}: {e}", d.name, k.name));
            }
        }
    }

    #[test]
    fn names_are_unique_and_ids() {
        let mut seen = std::collections::HashSet::new();
        for d in DOMAINS {
            assert!(seen.insert(d.name), "domain {} twice", d.name);
            assert!(Text::Id.check(d.name).is_ok());
            let mut keys = std::collections::HashSet::new();
            for k in d.keys {
                assert!(keys.insert(k.name), "{}.{} twice", d.name, k.name);
            }
        }
    }

    #[test]
    fn checks() {
        let accent = Kind::Text {
            rule: Text::Accent,
            optional: false,
        };
        assert!(accent.check(&Value::Text("blue".into())).is_ok());
        assert!(accent.check(&Value::Text("#3B6fd8".into())).is_ok());
        assert!(accent.check(&Value::Text("wallpaper".into())).is_ok());
        assert!(accent.check(&Value::Text("#3B6FD".into())).is_err());
        assert!(accent.check(&Value::Text("red".into())).is_err());
        assert!(accent.check(&Value::Text(String::new())).is_err());

        let id = Kind::Text {
            rule: Text::Id,
            optional: true,
        };
        assert!(id.check(&Value::Text(String::new())).is_ok());
        assert!(id.check(&Value::Text("../etc".into())).is_err());
        assert!(id.check(&Value::Text("a b".into())).is_err());

        let int = Kind::Int { min: 0, max: 300 };
        assert!(int.check(&Value::Int(300)).is_ok());
        assert!(int.check(&Value::Int(301)).is_err());
        assert!(int.check(&Value::Bool(true)).is_err());

        let path = Kind::Text {
            rule: Text::Path,
            optional: false,
        };
        assert!(path.check(&Value::Text("relative.png".into())).is_err());

        let list = Kind::List {
            item: Text::Any,
            max: 2,
        };
        assert!(
            list.check(&Value::List(vec!["a".into(), "b".into()]))
                .is_ok()
        );
        assert!(list.check(&Value::List(vec!["a".into(); 3])).is_err());
        assert!(list.check(&Value::List(vec!["a\nb".into()])).is_err());

        assert_eq!(Kind::Choice(&["a", "b"]).describe(), "choice a|b");
        assert_eq!(id.describe(), "id?");
    }
}
