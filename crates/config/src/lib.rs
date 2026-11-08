//! Configuration: `config.yaml` (or the file in `WHATSAPP_MCP_CONFIG`) and the
//! `WHATSAPP_*` environment variables, with the same rules as the Go bridge
//! and the Python server (see `docs/rust-1.0/interface.md`, section 2).
//!
//! - **File:** `~/.config/whatsapp-mcp/config.yaml`, or the path in
//!   `WHATSAPP_MCP_CONFIG`. A file named that way must exist; the default
//!   file is optional.
//! - **Precedence:** an environment variable that is set wins over the file.
//!   An empty one counts as unset.
//! - **Values** keep their exact text, so `0612` stays `0612`.
//!   `send_allowlist` may also be a YAML list; its entries are joined with `, `.
//! - **Errors:** an unknown setting, or a value of the wrong kind, is an error
//!   that gives the line number.
//! - **Permissions:** a file that others can read or change is refused,
//!   except on Windows.
//!
//! Unlike the Go and Python code, this never changes the process environment:
//! [`Config`] holds the result.

mod allowlist;
mod error;
mod logging;
mod parse;
mod startup;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

pub use allowlist::SendAllowlist;
pub use error::ConfigError;
pub use logging::{LogFormat, LogLevel, LogOutput};
pub use startup::Source;

/// The environment variable that names the configuration file.
pub const CONFIG_PATH_ENV: &str = "WHATSAPP_MCP_CONFIG";

/// The minimum length of the bridge token.
pub const MIN_BRIDGE_TOKEN_LEN: usize = 32;

/// A setting, with its name in the file and its environment variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Setting {
    BridgeToken,
    MediaDir,
    SendAllowlist,
    ReadOnly,
    LogContent,
    PairPhone,
    LogOutput,
    LogFormat,
    LogLevel,
}

impl Setting {
    pub const ALL: [Setting; 9] = [
        Setting::BridgeToken,
        Setting::MediaDir,
        Setting::SendAllowlist,
        Setting::ReadOnly,
        Setting::LogContent,
        Setting::PairPhone,
        Setting::LogOutput,
        Setting::LogFormat,
        Setting::LogLevel,
    ];

    /// The name in the configuration file.
    pub fn name(self) -> &'static str {
        match self {
            Setting::BridgeToken => "bridge_token",
            Setting::MediaDir => "media_dir",
            Setting::SendAllowlist => "send_allowlist",
            Setting::ReadOnly => "read_only",
            Setting::LogContent => "log_content",
            Setting::PairPhone => "pair_phone",
            Setting::LogOutput => "log_output",
            Setting::LogFormat => "log_format",
            Setting::LogLevel => "log_level",
        }
    }

    /// The environment variable.
    pub fn env(self) -> &'static str {
        match self {
            Setting::BridgeToken => "WHATSAPP_BRIDGE_TOKEN",
            Setting::MediaDir => "WHATSAPP_MEDIA_DIR",
            Setting::SendAllowlist => "WHATSAPP_SEND_ALLOWLIST",
            Setting::ReadOnly => "WHATSAPP_MCP_READ_ONLY",
            Setting::LogContent => "WHATSAPP_LOG_CONTENT",
            Setting::PairPhone => "WHATSAPP_PAIR_PHONE",
            Setting::LogOutput => "WHATSAPP_LOG_OUTPUT",
            Setting::LogFormat => "WHATSAPP_LOG_FORMAT",
            Setting::LogLevel => "WHATSAPP_LOG_LEVEL",
        }
    }

    pub fn from_name(name: &str) -> Option<Setting> {
        Setting::ALL.into_iter().find(|s| s.name() == name)
    }

    /// Whether the file may give this setting as a YAML list.
    fn may_be_list(self) -> bool {
        self == Setting::SendAllowlist
    }
}

/// Whether a yes/no value means yes: `1`, `true`, `yes` or `on`, in any case
/// and with spaces around it. Anything else, including an empty value, is no.
pub fn flag(value: &str) -> bool {
    let value = value.trim();
    ["1", "true", "yes", "on"]
        .iter()
        .any(|yes| value.eq_ignore_ascii_case(yes))
}

/// Reads a configuration file's text into its values, by environment variable
/// (as the Go and Python code do). A setting the file leaves empty is not in
/// the map. Used by the tests shared with the Go and Python code.
pub fn parse_config(text: &str) -> Result<BTreeMap<&'static str, String>, ConfigError> {
    Ok(parse::parse(text)?
        .into_iter()
        .map(|(setting, value)| (setting.env(), value))
        .collect())
}

/// The settings in effect: each environment variable that is set, and the
/// configuration file for the rest.
#[derive(Clone, Default)]
pub struct Config {
    file: Option<PathBuf>,
    /// The file that was looked for, also when it was not there.
    looked_for: Option<PathBuf>,
    values: BTreeMap<Setting, String>,
    /// The settings whose value comes from the environment.
    from_env: BTreeSet<Setting>,
    /// The home folder, for `~/` in `log_output`.
    home: Option<PathBuf>,
}

impl Config {
    /// Reads the configuration file and the environment of this process.
    pub fn load() -> Result<Config, ConfigError> {
        let env = |name: &'static str| match std::env::var(name) {
            Ok(value) => Ok(Some(value)),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => Err(ConfigError::NotUnicode(name)),
        };
        Config::load_from(env, std::env::home_dir().as_deref())
    }

    /// Like [`Config::load`], with the environment and home folder given:
    /// `env` returns a variable's value, or `None` when it is not set.
    pub fn load_from(
        env: impl Fn(&'static str) -> Result<Option<String>, ConfigError>,
        home: Option<&Path>,
    ) -> Result<Config, ConfigError> {
        let looked_for = config_path(&env, home)?;
        let (file, file_values) = match &looked_for {
            Some((path, required)) => match read_file(path, *required)? {
                Some(values) => (Some(path.clone()), values),
                None => (None, BTreeMap::new()),
            },
            None => (None, BTreeMap::new()),
        };

        let mut values = file_values;
        let mut from_env = BTreeSet::new();
        for setting in Setting::ALL {
            // A set environment variable wins; an empty one counts as unset.
            if let Some(value) = env(setting.env())?.filter(|v| !v.is_empty()) {
                values.insert(setting, value);
                from_env.insert(setting);
            }
        }
        Ok(Config {
            file,
            looked_for: looked_for.map(|(path, _)| path),
            values,
            from_env,
            home: home.map(Path::to_path_buf),
        })
    }

    /// Where a setting's value comes from.
    pub fn source(&self, setting: Setting) -> Source {
        if self.get(setting).is_none() {
            Source::Default
        } else if self.from_env.contains(&setting) {
            Source::Environment
        } else {
            Source::File
        }
    }

    /// The configuration file that was read, if any.
    pub fn file(&self) -> Option<&Path> {
        self.file.as_deref()
    }

    /// A setting's value, or `None` when it is not set or empty.
    pub fn get(&self, setting: Setting) -> Option<&str> {
        self.values
            .get(&setting)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    /// The bridge token. Both programs need it; the bridge refuses one that
    /// is shorter than [`MIN_BRIDGE_TOKEN_LEN`] bytes, as the Go bridge does.
    pub fn bridge_token(&self) -> Result<&str, ConfigError> {
        match self.get(Setting::BridgeToken) {
            Some(token) if token.len() >= MIN_BRIDGE_TOKEN_LEN => Ok(token),
            _ => Err(ConfigError::BridgeToken),
        }
    }

    /// The folder files may be sent from; `None` disables sending files.
    pub fn media_dir(&self) -> Option<&str> {
        self.get(Setting::MediaDir)
    }

    /// The chats the bridge may send to; `None` means no limit.
    pub fn send_allowlist(&self) -> Result<Option<SendAllowlist>, ConfigError> {
        SendAllowlist::parse(self.get(Setting::SendAllowlist).unwrap_or(""))
    }

    /// Whether the MCP server hides the tools that send messages.
    pub fn read_only(&self) -> bool {
        self.get(Setting::ReadOnly).is_some_and(flag)
    }

    /// Whether the bridge writes message content to its logs.
    pub fn log_content(&self) -> bool {
        self.get(Setting::LogContent).is_some_and(flag)
    }

    /// The phone number to link the bridge with a pairing code.
    pub fn pair_phone(&self) -> Option<&str> {
        self.get(Setting::PairPhone)
    }
}

/// Never shows the bridge token, so a `{:?}` in a log cannot leak it.
impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut map = f.debug_map();
        for (setting, value) in &self.values {
            if *setting == Setting::BridgeToken {
                map.entry(
                    &setting.name(),
                    &format_args!("<{} characters>", value.len()),
                );
            } else {
                map.entry(&setting.name(), value);
            }
        }
        map.finish()?;
        if let Some(file) = &self.file {
            write!(f, " from {}", file.display())?;
        }
        Ok(())
    }
}

/// The file to read and whether it must exist: only when
/// `WHATSAPP_MCP_CONFIG` names it. `~/` at its start means the home folder.
fn config_path(
    env: &impl Fn(&'static str) -> Result<Option<String>, ConfigError>,
    home: Option<&Path>,
) -> Result<Option<(PathBuf, bool)>, ConfigError> {
    let explicit = env(CONFIG_PATH_ENV)?.unwrap_or_default();
    let explicit = explicit.trim();
    if !explicit.is_empty() {
        let path = match (explicit.strip_prefix("~/"), home) {
            (Some(rest), Some(home)) => home.join(rest),
            _ if explicit == "~" => home.map_or_else(|| PathBuf::from("~"), Path::to_path_buf),
            _ => PathBuf::from(explicit),
        };
        return Ok(Some((path, true)));
    }
    Ok(home.map(|home| {
        (
            home.join(".config")
                .join("whatsapp-mcp")
                .join("config.yaml"),
            false,
        )
    }))
}

/// Reads and checks the file. `None` when an optional file does not exist.
fn read_file(
    path: &Path,
    required: bool,
) -> Result<Option<BTreeMap<Setting, String>>, ConfigError> {
    let meta = match std::fs::metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return if required {
                Err(ConfigError::Missing(path.to_path_buf()))
            } else {
                Ok(None)
            };
        }
        Err(source) => {
            return Err(ConfigError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    check_permissions(path, &meta)?;
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    parse::parse(&text).map(Some).map_err(|e| e.in_file(path))
}

/// The file holds the bridge token: refuse it when others can read or change
/// it. Windows permissions work differently; skip there.
#[cfg(unix)]
fn check_permissions(path: &Path, meta: &std::fs::Metadata) -> Result<(), ConfigError> {
    use std::os::unix::fs::PermissionsExt;
    if meta.permissions().mode() & 0o077 != 0 {
        return Err(ConfigError::Permissions(path.to_path_buf()));
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_permissions(_path: &Path, _meta: &std::fs::Metadata) -> Result<(), ConfigError> {
    Ok(())
}
