//! The fernctl <-> fern-init control protocol: one JSON object per line over
//! a Unix stream socket (`/run/fern/control`, mode 0600, root only).

use serde::{Deserialize, Serialize};

use crate::log::Record;

pub const SOCKET: &str = "run/fern/control";
/// Requests longer than this are rejected (and the connection dropped).
pub const MAX_REQUEST: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Request {
    Status {
        #[serde(default)]
        unit: Option<String>,
    },
    Start {
        unit: String,
    },
    Stop {
        unit: String,
    },
    Restart {
        unit: String,
    },
    Enable {
        unit: String,
    },
    Disable {
        unit: String,
    },
    Logs {
        #[serde(default)]
        unit: Option<String>,
        #[serde(default = "default_lines")]
        lines: usize,
    },
    BootTime,
    Reboot,
    Poweroff,
}

fn default_lines() -> usize {
    50
}

impl Request {
    /// Requests that change system state (vs. read-only queries).
    pub fn mutates(&self) -> bool {
        !matches!(
            self,
            Request::Status { .. } | Request::Logs { .. } | Request::BootTime
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitStatus {
    pub name: String,
    pub description: String,
    pub state: String,
    pub pid: Option<u32>,
    /// Milliseconds since the current state began.
    pub for_ms: u64,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Response {
    Ok { message: String },
    Error { message: String },
    Status { units: Vec<UnitStatus> },
    Logs { records: Vec<Record> },
}

/// Parses one request line.
///
/// # Errors
/// Oversized or malformed requests.
pub fn parse_request(line: &str) -> Result<Request, String> {
    if line.len() > MAX_REQUEST {
        return Err("request too long".into());
    }
    serde_json::from_str(line.trim()).map_err(|e| format!("bad request: {e}"))
}

pub fn encode<T: Serialize>(msg: &T) -> String {
    let mut s = serde_json::to_string(msg).unwrap_or_else(|_| "{}".into());
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        for r in [
            Request::Status { unit: None },
            Request::Start {
                unit: "getty".into(),
            },
            Request::Logs {
                unit: Some("a".into()),
                lines: 5,
            },
            Request::Poweroff,
        ] {
            assert_eq!(parse_request(&encode(&r)).unwrap(), r);
        }
        assert_eq!(
            parse_request(r#"{"cmd":"logs"}"#).unwrap(),
            Request::Logs {
                unit: None,
                lines: 50
            }
        );
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_request("").is_err());
        assert!(parse_request(r#"{"cmd":"format-disk"}"#).is_err());
        assert!(parse_request(r#"{"cmd":"start"}"#).is_err());
        assert!(parse_request(r#"{"cmd":"start","unit":"a","x":1}"#).is_err());
        assert!(parse_request(&"x".repeat(MAX_REQUEST + 1)).is_err());
    }

    #[test]
    fn mutating_requests() {
        assert!(!Request::Status { unit: None }.mutates());
        assert!(!Request::BootTime.mutates());
        assert!(Request::Reboot.mutates());
        assert!(Request::Enable { unit: "a".into() }.mutates());
    }
}
