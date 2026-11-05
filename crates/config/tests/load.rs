//! Loading: where the file is, precedence, permissions and the typed settings.
//! Ported from whatsapp-bridge/config_test.go and
//! whatsapp-mcp-server/tests/test_config.py.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use whatsapp_mcp_config::{CONFIG_PATH_ENV, Config, ConfigError, Setting, parse_config};

/// A fake environment for one test.
fn env(
    vars: &[(&str, &str)],
) -> impl Fn(&'static str) -> Result<Option<String>, ConfigError> + use<> {
    let vars: HashMap<String, String> = vars
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    move |name| Ok(vars.get(name).cloned())
}

fn write_config(dir: &Path, text: &str, mode: u32) -> PathBuf {
    let path = dir.join("config.yaml");
    std::fs::write(&path, text).unwrap();
    set_mode(&path, mode);
    path
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) {}

#[test]
fn environment_wins_and_empty_counts_as_unset() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_config(
        dir.path(),
        "media_dir: /from/file\npair_phone: 31600000000\nread_only: yes\n",
        0o600,
    );
    let config = Config::load_from(
        env(&[
            (CONFIG_PATH_ENV, path.to_str().unwrap()),
            ("WHATSAPP_MEDIA_DIR", "/from/env"), // set: wins over the file
            ("WHATSAPP_PAIR_PHONE", ""),         // empty: the file fills it in
        ]),
        None,
    )
    .unwrap();
    assert_eq!(config.file(), Some(path.as_path()));
    assert_eq!(config.media_dir(), Some("/from/env"));
    assert_eq!(config.pair_phone(), Some("31600000000"));
    assert!(config.read_only());
}

#[test]
fn environment_only() {
    let home = tempfile::tempdir().unwrap(); // no config file in it
    let config = Config::load_from(
        env(&[
            ("WHATSAPP_BRIDGE_TOKEN", &"t".repeat(32)),
            ("WHATSAPP_LOG_CONTENT", " On "),
            ("WHATSAPP_SEND_ALLOWLIST", "31612345678"),
        ]),
        Some(home.path()),
    )
    .unwrap();
    assert_eq!(config.file(), None);
    assert_eq!(config.bridge_token().unwrap(), "t".repeat(32));
    assert!(config.log_content());
    assert!(!config.read_only());
    assert_eq!(config.media_dir(), None);
    assert_eq!(
        config.send_allowlist().unwrap().unwrap().to_string(),
        "31612345678@s.whatsapp.net"
    );
}

#[test]
fn default_file_in_home_folder() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join(".config").join("whatsapp-mcp");
    std::fs::create_dir_all(&dir).unwrap();
    let path = write_config(&dir, "media_dir: /outbox\n", 0o600);
    let config = Config::load_from(env(&[]), Some(home.path())).unwrap();
    assert_eq!(config.file(), Some(path.as_path()));
    assert_eq!(config.media_dir(), Some("/outbox"));
}

#[test]
fn default_file_is_optional() {
    let home = tempfile::tempdir().unwrap();
    let config = Config::load_from(env(&[]), Some(home.path())).unwrap();
    assert_eq!(config.file(), None);
    // No home folder at all: no default file either.
    assert_eq!(Config::load_from(env(&[]), None).unwrap().file(), None);
}

#[test]
fn named_file_must_exist() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.yaml");
    let err =
        Config::load_from(env(&[(CONFIG_PATH_ENV, missing.to_str().unwrap())]), None).unwrap_err();
    assert!(matches!(err, ConfigError::Missing(_)), "{err:?}");
    assert!(err.to_string().contains("does not exist"), "{err}");
}

#[test]
fn named_file_with_tilde_and_spaces() {
    let home = tempfile::tempdir().unwrap();
    write_config(home.path(), "pair_phone: 3161\n", 0o600);
    let config = Config::load_from(
        env(&[(CONFIG_PATH_ENV, "  ~/config.yaml  ")]),
        Some(home.path()),
    )
    .unwrap();
    assert_eq!(config.pair_phone(), Some("3161"));
}

#[cfg(unix)]
#[test]
fn refuses_a_file_others_can_read_or_change() {
    for mode in [0o644, 0o620, 0o604] {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(dir.path(), "bridge_token: secret\n", mode);
        let err =
            Config::load_from(env(&[(CONFIG_PATH_ENV, path.to_str().unwrap())]), None).unwrap_err();
        assert!(err.to_string().contains("chmod 600"), "{mode:o}: {err}");
    }
}

#[test]
fn parse_errors_name_the_file_and_line() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_config(dir.path(), "read_only: yes\nsend_alowlist: 1\n", 0o600);
    let err = Config::load_from(env(&[(CONFIG_PATH_ENV, path.to_str().unwrap())]), None)
        .unwrap_err()
        .to_string();
    assert!(err.starts_with(path.to_str().unwrap()), "{err}");
    assert!(
        err.contains("line 2: unknown setting \"send_alowlist\""),
        "{err}"
    );
}

#[test]
fn bridge_token_needs_32_characters() {
    for (token, ok) in [
        ("", false),
        (&*"x".repeat(31), false),
        (&*"x".repeat(32), true),
    ] {
        let config = Config::load_from(env(&[("WHATSAPP_BRIDGE_TOKEN", token)]), None).unwrap();
        assert_eq!(
            config.bridge_token().is_ok(),
            ok,
            "{} characters",
            token.len()
        );
    }
    let err = Config::default().bridge_token().unwrap_err().to_string();
    assert!(err.contains("at least 32 characters"), "{err}");
}

#[test]
fn bad_allowlist_entry_is_an_error() {
    let config = Config::load_from(
        env(&[("WHATSAPP_SEND_ALLOWLIST", "31612345678, oops")]),
        None,
    )
    .unwrap();
    let err = config.send_allowlist().unwrap_err().to_string();
    assert!(err.contains("\"oops\""), "{err}");
}

#[test]
fn debug_output_hides_the_token() {
    let token = "0123456789abcdef0123456789abcdef";
    let config = Config::load_from(
        env(&[
            ("WHATSAPP_BRIDGE_TOKEN", token),
            ("WHATSAPP_MEDIA_DIR", "/x"),
        ]),
        None,
    )
    .unwrap();
    let debug = format!("{config:?}");
    assert!(!debug.contains(token), "{debug}");
    assert!(
        debug.contains("<32 characters>") && debug.contains("/x"),
        "{debug}"
    );
}

#[test]
fn example_file_is_valid() {
    // config.example.yaml must stay valid; its token is empty on purpose, so
    // a copied file cannot run with a known secret.
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../config.example.yaml");
    let values = parse_config(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(
        values.get("WHATSAPP_BRIDGE_TOKEN").map(String::as_str),
        Some("")
    );
    assert_eq!(
        values.get("WHATSAPP_MCP_READ_ONLY").map(String::as_str),
        Some("no")
    );
    let known: Vec<&str> = Setting::ALL.iter().map(|s| s.env()).collect();
    assert!(values.keys().all(|k| known.contains(k)), "{values:?}");
}

#[test]
fn settings_match_the_documented_names() {
    let pairs: Vec<(&str, &str)> = Setting::ALL.iter().map(|s| (s.name(), s.env())).collect();
    assert_eq!(
        pairs,
        [
            ("bridge_token", "WHATSAPP_BRIDGE_TOKEN"),
            ("media_dir", "WHATSAPP_MEDIA_DIR"),
            ("send_allowlist", "WHATSAPP_SEND_ALLOWLIST"),
            ("read_only", "WHATSAPP_MCP_READ_ONLY"),
            ("log_content", "WHATSAPP_LOG_CONTENT"),
            ("pair_phone", "WHATSAPP_PAIR_PHONE"),
        ]
    );
}

/// Stricter than the Go and Python code: these were silently accepted there.
#[test]
fn rust_refuses_ambiguous_files() {
    for (text, fragment) in [
        (
            "media_dir: /a\nmedia_dir: /b\n",
            "line 2: media_dir is set twice",
        ),
        (
            "media_dir: /a\n---\nmedia_dir: /b\n",
            "several YAML documents",
        ),
        ("media_dir: &d /a\npair_phone: *d\n", "aliases"),
    ] {
        let err = parse_config(text).unwrap_err().to_string();
        assert!(err.contains(fragment), "{text:?}: {err}");
    }
}
