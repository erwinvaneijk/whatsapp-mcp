//! Log lines: their format, which lines get through, and each output.

use chrono::{DateTime, FixedOffset};
use log::{Level, Log, Metadata, Record};
use whatsapp_mcp_config::{LogFormat, LogLevel, LogOutput};
use whatsapp_mcp_logging::{Logger, Program, clean, line, syslog_datagram};

fn time() -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339("2026-11-08T14:03:12.345+01:00").unwrap()
}

fn enabled(logger: &Logger, target: &str, level: Level) -> bool {
    logger.enabled(&Metadata::builder().target(target).level(level).build())
}

fn log(logger: &Logger, target: &str, level: Level, message: &str) {
    logger.log(
        &Record::builder()
            .target(target)
            .level(level)
            .args(format_args!("{message}"))
            .build(),
    );
}

fn stderr_logger(program: Program, level: LogLevel, rust_log: Option<&str>) -> Logger {
    Logger::new(
        program,
        &LogOutput::Stderr,
        LogFormat::Text,
        level,
        rust_log,
    )
    .unwrap()
}

#[test]
fn text_lines_have_time_level_and_source() {
    assert_eq!(
        line(
            LogFormat::Text,
            time(),
            Level::Info,
            "bridge",
            "✓ Connected to WhatsApp"
        ),
        "2026-11-08T14:03:12.345+01:00 INFO  bridge: ✓ Connected to WhatsApp"
    );
    assert_eq!(
        line(LogFormat::Text, time(), Level::Warn, "whatsapp-rust", "x"),
        "2026-11-08T14:03:12.345+01:00 WARN  whatsapp-rust: x"
    );
    assert_eq!(
        line(LogFormat::Text, time(), Level::Error, "serve", "x"),
        "2026-11-08T14:03:12.345+01:00 ERROR serve: x"
    );
}

#[test]
fn json_lines_are_one_object() {
    let text = line(LogFormat::Json, time(), Level::Info, "bridge", "say \"hi\"");
    assert!(!text.contains('\n'));
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "time": "2026-11-08T14:03:12.345+01:00",
            "level": "info",
            "source": "bridge",
            "message": "say \"hi\"",
        })
    );
}

#[test]
fn messages_stay_on_one_line_without_control_characters() {
    assert_eq!(
        clean("Scan:\nline two\r\n\tend\u{1b}[31m\u{7}"),
        "Scan: line two   end[31m"
    );
    assert_eq!(clean("Noor 👍 café"), "Noor 👍 café");
}

#[test]
fn syslog_datagrams() {
    assert_eq!(
        syslog_datagram(time(), Level::Info, 4242, "bridge: hello"),
        "<14>Nov  8 14:03:12 whatsapp-mcp[4242]: bridge: hello"
    );
    assert!(syslog_datagram(time(), Level::Error, 1, "x").starts_with("<11>"));
    assert!(syslog_datagram(time(), Level::Warn, 1, "x").starts_with("<12>"));
    assert!(syslog_datagram(time(), Level::Debug, 1, "x").starts_with("<15>"));
    // Cut at 2048 bytes, on a character boundary.
    let long = "é".repeat(2000);
    let datagram = syslog_datagram(time(), Level::Info, 1, &long);
    assert!(datagram.len() <= 2048 && datagram.len() > 2040);
}

#[test]
fn our_lines_from_the_level_libraries_from_warn() {
    let logger = stderr_logger(Program::Bridge, LogLevel::Info, None);
    assert!(enabled(&logger, "whatsapp_mcp_bridge::run", Level::Info));
    assert!(!enabled(&logger, "whatsapp_mcp_bridge::run", Level::Debug));
    assert!(enabled(&logger, "whatsapp_rust::client", Level::Warn));
    assert!(!enabled(&logger, "whatsapp_rust::client", Level::Info));

    let logger = stderr_logger(Program::Bridge, LogLevel::Debug, None);
    assert!(enabled(&logger, "whatsapp_mcp_store", Level::Debug));
    assert!(
        !enabled(&logger, "whatsapp_rust::client", Level::Info),
        "libraries stay at warn"
    );

    let logger = stderr_logger(Program::Bridge, LogLevel::Error, None);
    assert!(!enabled(&logger, "whatsapp_mcp_bridge", Level::Warn));
    assert!(!enabled(&logger, "whatsapp_rust", Level::Warn));
    assert!(enabled(&logger, "whatsapp_rust", Level::Error));

    // RUST_LOG goes on top, per module.
    let logger = stderr_logger(
        Program::Bridge,
        LogLevel::Info,
        Some("whatsapp_rust=debug,whatsapp_mcp=warn"),
    );
    assert!(enabled(&logger, "whatsapp_rust::send", Level::Debug));
    assert!(!enabled(&logger, "whatsapp_mcp_bridge::run", Level::Info));
    assert!(!enabled(&logger, "hyper", Level::Info));
}

#[test]
fn serve_never_logs_to_stdout() {
    let error = Logger::new(
        Program::Serve,
        &LogOutput::Stdout,
        LogFormat::Text,
        LogLevel::Info,
        None,
    )
    .err()
    .unwrap();
    assert!(error.contains("stdout carries the MCP protocol"), "{error}");
    assert!(
        Logger::new(
            Program::Bridge,
            &LogOutput::Stdout,
            LogFormat::Text,
            LogLevel::Info,
            None
        )
        .is_ok()
    );
}

#[test]
fn a_log_file_is_appended_to_and_private() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("whatsapp-mcp.log");
    std::fs::write(&path, "earlier line\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    let logger = Logger::new(
        Program::Serve,
        &LogOutput::File(path.clone()),
        LogFormat::Text,
        LogLevel::Info,
        None,
    )
    .unwrap();
    log(
        &logger,
        "whatsapp_mcp_server::tools",
        Level::Info,
        "first\nsecond",
    );
    log(&logger, "whatsapp_rust::client", Level::Info, "left out");
    log(&logger, "whatsapp_rust::client", Level::Warn, "a warning");
    logger.flush();

    let text = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "{text}");
    assert_eq!(lines[0], "earlier line");
    assert!(
        lines[1].ends_with(" INFO  serve: first second"),
        "{}",
        lines[1]
    );
    assert!(
        lines[2].ends_with(" WARN  whatsapp-rust: a warning"),
        "{}",
        lines[2]
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    let missing = dir.path().join("no-such-folder").join("x.log");
    let error = Logger::new(
        Program::Bridge,
        &LogOutput::File(missing),
        LogFormat::Text,
        LogLevel::Info,
        None,
    )
    .err()
    .unwrap();
    assert!(error.starts_with("Cannot open the log file"), "{error}");
}

#[cfg(unix)]
#[test]
fn syslog_gets_one_datagram_per_line() {
    use std::os::unix::net::UnixDatagram;
    let dir = tempfile::tempdir().unwrap();
    let socket_path = dir.path().join("log.sock");
    let daemon = UnixDatagram::bind(&socket_path).unwrap();
    daemon
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();

    let logger = Logger::with_syslog_socket(
        Program::Bridge,
        &socket_path,
        LogFormat::Text,
        LogLevel::Info,
    )
    .unwrap();
    log(
        &logger,
        "whatsapp_mcp_bridge::run",
        Level::Warn,
        "store\u{1b}[31m failed",
    );
    let mut buffer = [0u8; 4096];
    let n = daemon.recv(&mut buffer).unwrap();
    let datagram = std::str::from_utf8(&buffer[..n]).unwrap();
    let pid = std::process::id();
    assert!(datagram.starts_with("<12>"), "{datagram}");
    assert!(
        datagram.ends_with(&format!(" whatsapp-mcp[{pid}]: bridge: store[31m failed")),
        "{datagram}"
    );

    let logger = Logger::with_syslog_socket(
        Program::Serve,
        &socket_path,
        LogFormat::Json,
        LogLevel::Info,
    )
    .unwrap();
    log(&logger, "whatsapp_mcp_server", Level::Info, "hi");
    let n = daemon.recv(&mut buffer).unwrap();
    let datagram = std::str::from_utf8(&buffer[..n]).unwrap();
    let json = datagram.split_once("]: ").unwrap().1;
    let value: serde_json::Value = serde_json::from_str(json).unwrap();
    assert_eq!(value["source"], "serve");
    assert_eq!(value["message"], "hi");
}

#[cfg(windows)]
#[test]
fn no_syslog_on_windows() {
    let error = Logger::new(
        Program::Bridge,
        &LogOutput::Syslog,
        LogFormat::Text,
        LogLevel::Info,
        None,
    )
    .err()
    .unwrap();
    assert!(error.contains("cannot be syslog on Windows"), "{error}");
}
