//! The logging settings: where log lines go, their format and the level.
//! Values are read case-insensitively, with spaces around them ignored; an
//! unknown value is an error that names the setting.

use std::path::PathBuf;

use crate::{Config, ConfigError, Setting};

/// Where log lines go (`log_output`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogOutput {
    Stderr,
    Stdout,
    /// The local syslog (not on Windows).
    Syslog,
    /// A file, appended to.
    File(PathBuf),
}

/// How a log line is written (`log_format`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    /// `2026-11-08T14:03:12.345+01:00 INFO  bridge: text`
    Text,
    /// One JSON object per line.
    Json,
}

/// The least severe lines that are logged (`log_level`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
}

impl Config {
    /// Where log lines go; `stderr` when not set. A file path must be
    /// absolute or start with `~/`: a relative one would depend on the
    /// folder the program happens to be started in.
    pub fn log_output(&self) -> Result<LogOutput, ConfigError> {
        let Some(value) = self.get(Setting::LogOutput).map(str::trim) else {
            return Ok(LogOutput::Stderr);
        };
        match value.to_ascii_lowercase().as_str() {
            "stderr" => return Ok(LogOutput::Stderr),
            "stdout" => return Ok(LogOutput::Stdout),
            "syslog" => return Ok(LogOutput::Syslog),
            _ => {}
        }
        let path = match (value.strip_prefix("~/"), &self.home) {
            (Some(rest), Some(home)) => home.join(rest),
            _ => PathBuf::from(value),
        };
        if path.is_absolute() {
            Ok(LogOutput::File(path))
        } else {
            Err(invalid(
                Setting::LogOutput,
                value,
                "stderr, stdout, syslog, or an absolute file path (or one starting with ~/)",
            ))
        }
    }

    /// The format of log lines; `text` when not set.
    pub fn log_format(&self) -> Result<LogFormat, ConfigError> {
        match self.lowercase(Setting::LogFormat).as_deref() {
            None | Some("text") => Ok(LogFormat::Text),
            Some("json") => Ok(LogFormat::Json),
            Some(_) => Err(self.invalid_value(Setting::LogFormat, "text or json")),
        }
    }

    /// The log level; `info` when not set.
    pub fn log_level(&self) -> Result<LogLevel, ConfigError> {
        match self.lowercase(Setting::LogLevel).as_deref() {
            Some("error") => Ok(LogLevel::Error),
            Some("warn" | "warning") => Ok(LogLevel::Warn),
            None | Some("info") => Ok(LogLevel::Info),
            Some("debug") => Ok(LogLevel::Debug),
            Some(_) => Err(self.invalid_value(Setting::LogLevel, "error, warn, info or debug")),
        }
    }

    fn lowercase(&self, setting: Setting) -> Option<String> {
        self.get(setting).map(|v| v.trim().to_ascii_lowercase())
    }

    fn invalid_value(&self, setting: Setting, expected: &str) -> ConfigError {
        invalid(setting, self.get(setting).unwrap_or_default(), expected)
    }
}

fn invalid(setting: Setting, value: &str, expected: &str) -> ConfigError {
    let value: String = value.chars().filter(|c| !c.is_control()).collect();
    ConfigError::invalid(format!(
        "{} (or {} in the configuration file) is {value:?}; use {expected}",
        setting.env(),
        setting.name()
    ))
}
