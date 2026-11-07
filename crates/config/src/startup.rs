//! The settings as `serve` and `bridge` log them on start: each value and
//! where it comes from, so a wrong setup shows in the log. Never the bridge
//! token or the pairing phone number, only whether they are set.

use crate::{Config, LogFormat, LogOutput, MIN_BRIDGE_TOKEN_LEN, SendAllowlist, Setting};

/// Where a setting's value comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Its environment variable.
    Environment,
    /// The configuration file.
    File,
    /// Not set: the default.
    Default,
}

impl Config {
    /// The lines that describe the configuration file and `settings`, for
    /// the log on start.
    pub fn startup_lines(&self, settings: &[Setting]) -> Vec<String> {
        let mut lines = vec![self.file_line()];
        for &setting in settings {
            let source = match self.source(setting) {
                Source::Environment => format!("from {}", setting.env()),
                Source::File => "from the configuration file".to_string(),
                Source::Default => "default".to_string(),
            };
            lines.push(format!(
                "  {}: {} ({source})",
                setting.name(),
                self.shown_value(setting)
            ));
        }
        lines
    }

    fn file_line(&self) -> String {
        match (&self.file, &self.looked_for) {
            (Some(file), _) => format!(
                "Configuration file: {} (environment variables take precedence)",
                clean(&file.display().to_string())
            ),
            (None, Some(path)) => format!(
                "Configuration file: none ({} not found)",
                clean(&path.display().to_string())
            ),
            (None, None) => "Configuration file: none".to_string(),
        }
    }

    /// A setting's value as logged: secrets only as set or not set.
    fn shown_value(&self, setting: Setting) -> String {
        let value = self.get(setting);
        match setting {
            Setting::BridgeToken => match value {
                None => "not set".into(),
                Some(token) if token.len() < MIN_BRIDGE_TOKEN_LEN => {
                    format!("too short (needs {MIN_BRIDGE_TOKEN_LEN} characters or more)")
                }
                Some(_) => "set".into(),
            },
            Setting::PairPhone => if value.is_some() { "set" } else { "not set" }.into(),
            Setting::ReadOnly => on_off(self.read_only()),
            Setting::LogContent => on_off(self.log_content()),
            Setting::MediaDir => match value {
                Some(dir) => clean(dir),
                None => "not set (sending files is disabled)".into(),
            },
            Setting::SendAllowlist => match SendAllowlist::parse(value.unwrap_or("")) {
                Ok(Some(list)) if list.len() == 1 => "1 chat".into(),
                Ok(Some(list)) => format!("{} chats", list.len()),
                Ok(None) => "not set (all chats)".into(),
                Err(e) => format!("invalid: {}", clean(&e.to_string())),
            },
            Setting::LogOutput => match self.log_output() {
                Ok(LogOutput::Stderr) => "stderr".into(),
                Ok(LogOutput::Stdout) => "stdout".into(),
                Ok(LogOutput::Syslog) => "syslog".into(),
                Ok(LogOutput::File(path)) => clean(&path.display().to_string()),
                Err(_) => "invalid".into(),
            },
            Setting::LogFormat => match self.log_format() {
                Ok(LogFormat::Text) => "text".into(),
                Ok(LogFormat::Json) => "json".into(),
                Err(_) => "invalid".into(),
            },
            Setting::LogLevel => match self.log_level() {
                Ok(level) => format!("{level:?}").to_ascii_lowercase(),
                Err(_) => "invalid".into(),
            },
        }
    }
}

fn on_off(on: bool) -> String {
    if on { "on" } else { "off" }.into()
}

/// Removes control characters, so a value cannot inject terminal escapes.
fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}
