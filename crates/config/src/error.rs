use std::fmt;
use std::path::PathBuf;

/// Why the configuration could not be used. The bridge and the MCP server
/// both stop on any of these, with the message on stderr.
#[derive(Debug)]
#[non_exhaustive]
pub enum ConfigError {
    /// The file named in `WHATSAPP_MCP_CONFIG` does not exist.
    Missing(PathBuf),
    /// The file can be read or changed by other users (not checked on Windows).
    Permissions(PathBuf),
    /// The file could not be read.
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The file is not valid, or not valid settings. `message` says why, with
    /// the line number where there is one.
    Invalid {
        path: Option<PathBuf>,
        message: String,
    },
    /// An environment variable holds text that is not valid Unicode.
    NotUnicode(&'static str),
    /// The bridge token is missing or shorter than 32 characters.
    BridgeToken,
    /// A send allowlist entry is not a phone number or a JID.
    Allowlist { entry: String, reason: String },
}

impl ConfigError {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid {
            path: None,
            message: message.into(),
        }
    }

    /// Adds the file name to a parse error.
    pub(crate) fn in_file(self, file: &std::path::Path) -> Self {
        match self {
            Self::Invalid {
                path: None,
                message,
            } => Self::Invalid {
                path: Some(file.to_path_buf()),
                message,
            },
            other => other,
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(path) => {
                write!(f, "configuration file {} does not exist", path.display())
            }
            Self::Permissions(path) => write!(
                f,
                "{0} can be read or changed by other users; run: chmod 600 {0}",
                path.display()
            ),
            Self::Io { path, source } => {
                write!(f, "configuration file {}: {source}", path.display())
            }
            Self::Invalid {
                path: Some(path),
                message,
            } => write!(f, "{}: {message}", path.display()),
            Self::Invalid {
                path: None,
                message,
            } => f.write_str(message),
            Self::NotUnicode(name) => write!(f, "{name} is not valid Unicode text"),
            Self::BridgeToken => f.write_str(
                "WHATSAPP_BRIDGE_TOKEN (or bridge_token in the configuration file) must be set to a random secret of at least 32 characters",
            ),
            Self::Allowlist { entry, reason } => {
                write!(f, "WHATSAPP_SEND_ALLOWLIST entry {entry:?}: {reason}")
            }
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
