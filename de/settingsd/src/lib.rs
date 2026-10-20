//! fern-settingsd: every component reads and writes settings through this
//! daemon, never the files (CLAUDE.md 6.10). Typed domains
//! ([`schema`]), validated writes, TOML storage ([`store`]) and the D-Bus
//! interface `org.fern.Settings1` ([`bus`]); docs/protocols/settings.md.

pub mod bus;
pub mod schema;
pub mod store;

use zbus::zvariant::{self, OwnedValue};

/// A setting's value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Text(String),
    List(Vec<String>),
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Bool(_) => "a bool",
            Value::Int(_) => "an integer",
            Value::Text(_) => "text",
            Value::List(_) => "a list of text",
        }
    }

    /// From a TOML value (`None` for other TOML types).
    pub fn from_toml(v: &toml::Value) -> Option<Value> {
        Some(match v {
            toml::Value::Boolean(b) => Value::Bool(*b),
            toml::Value::Integer(i) => Value::Int(*i),
            toml::Value::String(s) => Value::Text(s.clone()),
            toml::Value::Array(a) => Value::List(
                a.iter()
                    .map(|x| x.as_str().map(str::to_owned))
                    .collect::<Option<_>>()?,
            ),
            _ => return None,
        })
    }

    pub fn to_toml(&self) -> toml::Value {
        match self {
            Value::Bool(b) => toml::Value::Boolean(*b),
            Value::Int(i) => toml::Value::Integer(*i),
            Value::Text(s) => toml::Value::String(s.clone()),
            Value::List(l) => {
                toml::Value::Array(l.iter().cloned().map(toml::Value::String).collect())
            }
        }
    }

    /// From a D-Bus variant: `b`, any integer type, `s`, or `as`.
    pub fn from_variant(v: &zvariant::Value<'_>) -> Option<Value> {
        use zvariant::Value as V;
        Some(match v {
            V::Bool(b) => Value::Bool(*b),
            V::I64(i) => Value::Int(*i),
            V::I32(i) => Value::Int(i64::from(*i)),
            V::I16(i) => Value::Int(i64::from(*i)),
            V::U8(i) => Value::Int(i64::from(*i)),
            V::U16(i) => Value::Int(i64::from(*i)),
            V::U32(i) => Value::Int(i64::from(*i)),
            V::U64(i) => Value::Int(i64::try_from(*i).ok()?),
            V::Str(s) => Value::Text(s.as_str().to_owned()),
            V::Value(inner) => return Value::from_variant(inner),
            V::Array(a) => Value::List(
                a.iter()
                    .map(|x| match x {
                        V::Str(s) => Some(s.as_str().to_owned()),
                        _ => None,
                    })
                    .collect::<Option<_>>()?,
            ),
            _ => return None,
        })
    }

    /// As a D-Bus variant: `b`, `x`, `s` or `as`.
    ///
    /// # Panics
    /// Never: only values holding file descriptors fail to convert.
    pub fn to_variant(&self) -> OwnedValue {
        let v = match self {
            Value::Bool(b) => zvariant::Value::from(*b),
            Value::Int(i) => zvariant::Value::from(*i),
            Value::Text(s) => zvariant::Value::from(s.clone()),
            Value::List(l) => zvariant::Value::from(l.clone()),
        };
        // Only file descriptors make this fail.
        OwnedValue::try_from(v).expect("no file descriptors")
    }

    /// Parses text as `kind` (the `Describe` type string), for the CLI.
    ///
    /// # Errors
    /// When the text is not a value of that type.
    pub fn parse(kind: &str, text: &str) -> Result<Value, String> {
        let base = kind.split(' ').next().unwrap_or("");
        match base {
            "bool" => match text {
                "true" | "on" | "yes" => Ok(Value::Bool(true)),
                "false" | "off" | "no" => Ok(Value::Bool(false)),
                _ => Err(format!("{text:?} is not true or false")),
            },
            "int" => text
                .parse()
                .map(Value::Int)
                .map_err(|_| format!("{text:?} is not an integer")),
            "list" => Ok(Value::List(if text.is_empty() {
                Vec::new()
            } else {
                text.split(',').map(|s| s.trim().to_owned()).collect()
            })),
            _ => Ok(Value::Text(text.to_owned())),
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(i) => write!(f, "{i}"),
            Value::Text(s) => write!(f, "{s}"),
            Value::List(l) => write!(f, "{}", l.join(",")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_round_trip() {
        for v in [
            Value::Bool(true),
            Value::Int(-5),
            Value::Text("x".into()),
            Value::List(vec!["a".into(), "b".into()]),
            Value::List(Vec::new()),
        ] {
            let ov = v.to_variant();
            assert_eq!(Value::from_variant(&ov), Some(v.clone()));
            assert_eq!(Value::from_toml(&v.to_toml()), Some(v));
        }
        assert_eq!(
            Value::from_variant(&zvariant::Value::from(7u32)),
            Some(Value::Int(7))
        );
        assert_eq!(Value::from_variant(&zvariant::Value::from(1.5f64)), None);
    }

    #[test]
    fn parse_text() {
        assert_eq!(Value::parse("bool", "on"), Ok(Value::Bool(true)));
        assert!(Value::parse("bool", "maybe").is_err());
        assert_eq!(Value::parse("int 0..300", "12"), Ok(Value::Int(12)));
        assert_eq!(
            Value::parse("list id", "a, b"),
            Ok(Value::List(vec!["a".into(), "b".into()]))
        );
        assert_eq!(Value::parse("list id", ""), Ok(Value::List(Vec::new())));
        assert_eq!(Value::parse("choice a|b", "a"), Ok(Value::Text("a".into())));
    }
}
