//! Log lines for `serve` and `bridge`: every line has a time, a level and
//! its source, the same for our own code and for the libraries (through the
//! `log` facade). Where lines go, their format and the level come from the
//! settings `log_output`, `log_format` and `log_level`; `RUST_LOG` is
//! applied on top, per module, for debugging.
//!
//! ```text
//! 2026-11-08T14:03:12.345+01:00 INFO  bridge: ✓ Connected to WhatsApp
//! 2026-11-08T14:03:13.020+01:00 WARN  whatsapp-rust: Failed processing group plaintext (batch): …
//! ```
//!
//! Control characters are removed from every message, so a message cannot
//! inject terminal escapes or start a line that looks like one of ours.

use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

use chrono::{DateTime, FixedOffset, Local};
use log::{Level, LevelFilter, Log, Metadata, Record};
use whatsapp_mcp_config::{Config, LogFormat, LogLevel, LogOutput};

/// Which command is logging; the source shown for our own lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Program {
    Serve,
    Bridge,
}

impl Program {
    pub fn name(self) -> &'static str {
        match self {
            Program::Serve => "serve",
            Program::Bridge => "bridge",
        }
    }
}

/// The prefix of our own crates' module paths (`whatsapp_mcp_bridge::run`).
const OWN_CRATES: &str = "whatsapp_mcp";

/// The name syslog shows for our lines.
const SYSLOG_TAG: &str = "whatsapp-mcp";

/// The longest syslog message sent, in bytes; longer ones are cut. Local
/// syslog daemons accept at least this much in one datagram.
const SYSLOG_MAX: usize = 2048;

/// Sets up logging from the settings, for the rest of the process. Fails on
/// a setting `serve` cannot use (`stdout`), an output that cannot be opened,
/// or syslog on Windows.
pub fn init(program: Program, config: &Config) -> Result<(), String> {
    let output = config.log_output().map_err(|e| e.to_string())?;
    let format = config.log_format().map_err(|e| e.to_string())?;
    let level = config.log_level().map_err(|e| e.to_string())?;
    let rust_log = std::env::var("RUST_LOG").ok();
    let logger = Logger::new(program, &output, format, level, rust_log.as_deref())?;
    let max = logger.filter.filter();
    log::set_boxed_logger(Box::new(logger)).map_err(|e| e.to_string())?;
    log::set_max_level(max);
    Ok(())
}

/// The logger: a filter, a format and where lines go.
pub struct Logger {
    program: Program,
    format: LogFormat,
    filter: env_filter::Filter,
    sink: Sink,
}

enum Sink {
    Stderr,
    Stdout,
    File(Mutex<std::fs::File>),
    #[cfg(unix)]
    Syslog(std::os::unix::net::UnixDatagram),
}

impl Logger {
    /// A logger for `program`, writing to `output`. Our own crates log from
    /// `level`; libraries from `warn` (or `error`, when `level` is `error`).
    /// `rust_log` (the `RUST_LOG` value) is applied on top, per module.
    pub fn new(
        program: Program,
        output: &LogOutput,
        format: LogFormat,
        level: LogLevel,
        rust_log: Option<&str>,
    ) -> Result<Logger, String> {
        let sink = match output {
            LogOutput::Stderr => Sink::Stderr,
            LogOutput::Stdout if program == Program::Serve => {
                return Err(
                    "WHATSAPP_LOG_OUTPUT (or log_output) cannot be stdout for serve: stdout carries the MCP protocol; use stderr, syslog or a file"
                        .into(),
                );
            }
            LogOutput::Stdout => Sink::Stdout,
            LogOutput::File(path) => {
                Sink::File(Mutex::new(open_private(path).map_err(|e| {
                    format!("Cannot open the log file {}: {e}", path.display())
                })?))
            }
            LogOutput::Syslog => open_syslog()?,
        };
        let own = level_filter(level);
        let mut builder = env_filter::Builder::new();
        builder
            .filter_level(own.min(LevelFilter::Warn))
            .filter_module(OWN_CRATES, own);
        if let Some(rust_log) = rust_log.filter(|v| !v.trim().is_empty()) {
            builder.parse(rust_log);
        }
        Ok(Logger {
            program,
            format,
            filter: builder.build(),
            sink,
        })
    }

    /// A logger that writes to a syslog socket at `socket`, for tests.
    #[cfg(unix)]
    pub fn with_syslog_socket(
        program: Program,
        socket: &Path,
        format: LogFormat,
        level: LogLevel,
    ) -> Result<Logger, String> {
        let mut logger = Logger::new(program, &LogOutput::Stderr, format, level, None)?;
        logger.sink = Sink::Syslog(connect_syslog(socket).map_err(|e| e.to_string())?);
        Ok(logger)
    }

    /// Where a record comes from: the program for our own crates, else the
    /// library's crate name (`whatsapp-rust`).
    fn source<'a>(&self, target: &'a str) -> std::borrow::Cow<'a, str> {
        if target.starts_with(OWN_CRATES) {
            return self.program.name().into();
        }
        let krate = target.split("::").next().unwrap_or(target);
        if krate.contains('_') {
            krate.replace('_', "-").into()
        } else {
            krate.into()
        }
    }
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        self.filter.enabled(metadata)
    }

    fn log(&self, record: &Record<'_>) {
        if !self.filter.matches(record) {
            return;
        }
        let source = self.source(record.target());
        let message = clean(&record.args().to_string());
        let now = Local::now().fixed_offset();
        let _ = match &self.sink {
            Sink::Stderr => writeln!(
                std::io::stderr().lock(),
                "{}",
                line(self.format, now, record.level(), &source, &message)
            ),
            Sink::Stdout => writeln!(
                std::io::stdout().lock(),
                "{}",
                line(self.format, now, record.level(), &source, &message)
            ),
            Sink::File(file) => {
                let mut file = file.lock().unwrap_or_else(|e| e.into_inner());
                writeln!(
                    file,
                    "{}",
                    line(self.format, now, record.level(), &source, &message)
                )
            }
            #[cfg(unix)]
            Sink::Syslog(socket) => {
                let body = match self.format {
                    LogFormat::Text => format!("{source}: {message}"),
                    LogFormat::Json => json(now, record.level(), &source, &message),
                };
                let datagram = syslog_datagram(now, record.level(), std::process::id(), &body);
                socket.send(datagram.as_bytes()).map(|_| ())
            }
        };
    }

    fn flush(&self) {
        match &self.sink {
            Sink::Stderr => {
                let _ = std::io::stderr().flush();
            }
            Sink::Stdout => {
                let _ = std::io::stdout().flush();
            }
            Sink::File(file) => {
                let _ = file.lock().unwrap_or_else(|e| e.into_inner()).flush();
            }
            #[cfg(unix)]
            Sink::Syslog(_) => {}
        }
    }
}

fn level_filter(level: LogLevel) -> LevelFilter {
    match level {
        LogLevel::Error => LevelFilter::Error,
        LogLevel::Warn => LevelFilter::Warn,
        LogLevel::Info => LevelFilter::Info,
        LogLevel::Debug => LevelFilter::Debug,
    }
}

/// One log line in `format`, without the line break.
pub fn line(
    format: LogFormat,
    time: DateTime<FixedOffset>,
    level: Level,
    source: &str,
    message: &str,
) -> String {
    match format {
        LogFormat::Text => format!(
            "{} {:<5} {source}: {message}",
            time.format("%Y-%m-%dT%H:%M:%S%.3f%:z"),
            level
        ),
        LogFormat::Json => json(time, level, source, message),
    }
}

fn json(time: DateTime<FixedOffset>, level: Level, source: &str, message: &str) -> String {
    serde_json::json!({
        "time": time.format("%Y-%m-%dT%H:%M:%S%.3f%:z").to_string(),
        "level": level.as_str().to_ascii_lowercase(),
        "source": source,
        "message": message,
    })
    .to_string()
}

/// A syslog datagram as the local daemon expects it (RFC 3164, facility
/// `user`): `<PRI>Mmm dd hh:mm:ss whatsapp-mcp[pid]: body`, at most
/// [`SYSLOG_MAX`] bytes.
pub fn syslog_datagram(time: DateTime<FixedOffset>, level: Level, pid: u32, body: &str) -> String {
    const USER: u8 = 1;
    let severity: u8 = match level {
        Level::Error => 3,
        Level::Warn => 4,
        Level::Info => 6,
        Level::Debug | Level::Trace => 7,
    };
    let mut datagram = format!(
        "<{}>{} {SYSLOG_TAG}[{pid}]: {body}",
        USER * 8 + severity,
        time.format("%b %e %H:%M:%S")
    );
    if datagram.len() > SYSLOG_MAX {
        let mut end = SYSLOG_MAX;
        while !datagram.is_char_boundary(end) {
            end -= 1;
        }
        datagram.truncate(end);
    }
    datagram
}

/// The message without control characters; line breaks and tabs become
/// spaces, so one record stays one line.
pub fn clean(message: &str) -> String {
    message
        .chars()
        .filter_map(|c| match c {
            '\n' | '\r' | '\t' => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

/// Opens a log file for appending, created (or made) readable by the owner
/// only, as the store is.
fn open_private(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}

/// The local syslog sockets: Linux, macOS, the BSDs.
#[cfg(unix)]
const SYSLOG_SOCKETS: [&str; 3] = ["/dev/log", "/var/run/syslog", "/var/run/log"];

#[cfg(unix)]
fn open_syslog() -> Result<Sink, String> {
    for path in SYSLOG_SOCKETS {
        if let Ok(socket) = connect_syslog(Path::new(path)) {
            return Ok(Sink::Syslog(socket));
        }
    }
    Err(format!(
        "WHATSAPP_LOG_OUTPUT (or log_output) is syslog, but no local syslog socket was found ({})",
        SYSLOG_SOCKETS.join(", ")
    ))
}

#[cfg(not(unix))]
fn open_syslog() -> Result<Sink, String> {
    Err("WHATSAPP_LOG_OUTPUT (or log_output) cannot be syslog on Windows; use stderr, stdout or a file".into())
}

#[cfg(unix)]
fn connect_syslog(path: &Path) -> std::io::Result<std::os::unix::net::UnixDatagram> {
    let socket = std::os::unix::net::UnixDatagram::unbound()?;
    socket.connect(path)?;
    Ok(socket)
}
