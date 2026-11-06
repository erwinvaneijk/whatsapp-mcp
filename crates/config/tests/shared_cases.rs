//! The configuration cases shared with the Go bridge and the Python server
//! (`parity/config-cases.json`): all three must follow the same rules.

use std::collections::BTreeMap;

use serde::Deserialize;
use whatsapp_mcp_config::{SendAllowlist, flag, parse_config};

#[derive(Deserialize)]
struct Cases {
    files: Vec<FileCase>,
    allowlists: Vec<AllowlistCase>,
    flags: Flags,
}

#[derive(Deserialize)]
struct FileCase {
    name: String,
    yaml: String,
    values: Option<BTreeMap<String, String>>,
    error_contains: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct AllowlistCase {
    value: String,
    #[serde(default)]
    jids: Option<Vec<String>>,
    #[serde(default)]
    error: bool,
}

#[derive(Deserialize)]
struct Flags {
    on: Vec<String>,
    off: Vec<String>,
}

fn cases() -> Cases {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../parity/config-cases.json"
    );
    let text = std::fs::read_to_string(path).expect("parity/config-cases.json");
    serde_json::from_str(&text).expect("valid config-cases.json")
}

#[test]
fn files() {
    for case in cases().files {
        let result = parse_config(&case.yaml);
        match (&case.values, &case.error_contains) {
            (Some(want), None) => {
                let got: BTreeMap<String, String> = result
                    .unwrap_or_else(|e| panic!("{}: unexpected error: {e}", case.name))
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v))
                    .collect();
                assert_eq!(&got, want, "{}", case.name);
            }
            (None, Some(fragments)) => {
                let message = match result {
                    Ok(values) => panic!("{}: want an error, got {values:?}", case.name),
                    Err(e) => e.to_string(),
                };
                for fragment in fragments {
                    assert!(
                        message.contains(fragment.as_str()),
                        "{}: error {message:?} should contain {fragment:?}",
                        case.name
                    );
                }
            }
            _ => panic!("{}: give either values or error_contains", case.name),
        }
    }
}

#[test]
fn allowlists() {
    for case in cases().allowlists {
        let result = SendAllowlist::parse(&case.value);
        if case.error {
            assert!(result.is_err(), "{:?}: want an error", case.value);
            continue;
        }
        let got = result
            .unwrap_or_else(|e| panic!("{:?}: unexpected error: {e}", case.value))
            .map(|list| list.jids().map(str::to_string).collect::<Vec<_>>());
        assert_eq!(got, case.jids, "{:?}", case.value);
    }
}

#[test]
fn flags() {
    let flags = cases().flags;
    for value in flags.on {
        assert!(flag(&value), "{value:?} must mean yes");
    }
    for value in flags.off {
        assert!(!flag(&value), "{value:?} must mean no");
    }
}
