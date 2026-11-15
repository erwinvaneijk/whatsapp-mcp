//! `whatsapp-mcp service`: runs `whatsapp-mcp bridge` as a user service, so
//! it keeps running without a terminal and starts again after logging in.
//! macOS: a launchd agent. Linux: a systemd user unit. Windows: not yet;
//! the README explains a scheduled task.
//!
//! The file is written from this binary's own path and an absolute store
//! folder. The service gets no environment variables but
//! `WHATSAPP_MCP_CONFIG`, so the settings must be in the configuration file.
//!
//! Each platform's file is written by a plain function, tested on every
//! platform; only the commands that install it are per platform.

// These commands answer on stdout; they never run as `serve`, whose stdout
// carries the MCP protocol.
#![allow(clippy::print_stdout)]

use std::path::{Path, PathBuf};
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::process::Command;

use whatsapp_mcp_config::{CONFIG_PATH_ENV, Config, ConfigError};

/// The launchd label, also the name of the agent's file.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const LAUNCHD_LABEL: &str = "io.github.erwinvaneijk.whatsapp-mcp.bridge";

/// The systemd user unit's name.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub const SYSTEMD_UNIT: &str = "whatsapp-mcp-bridge.service";

/// What the service runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    /// This program, as an absolute path.
    pub exe: PathBuf,
    /// The store folder, as an absolute path.
    pub store: PathBuf,
    /// `WHATSAPP_MCP_CONFIG`, when set at install time.
    pub config_file: Option<PathBuf>,
    /// macOS: where launchd writes the bridge's stderr.
    pub log_file: Option<PathBuf>,
}

/// The launchd agent. It starts at login, and again when the bridge stops
/// with an error.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn launchd_plist(service: &Service) -> String {
    let mut env = String::new();
    if let Some(config) = &service.config_file {
        env = format!(
            "  <key>EnvironmentVariables</key>\n  <dict>\n    <key>{CONFIG_PATH_ENV}</key>\n    <string>{}</string>\n  </dict>\n",
            xml(&config.display().to_string())
        );
    }
    let mut log = String::new();
    if let Some(file) = &service.log_file {
        log = format!(
            "  <key>StandardErrorPath</key>\n  <string>{}</string>\n",
            xml(&file.display().to_string())
        );
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LAUNCHD_LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
    <string>bridge</string>
    <string>--store</string>
    <string>{}</string>
  </array>
{env}{log}  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <dict>
    <key>SuccessfulExit</key>
    <false/>
  </dict>
  <key>ThrottleInterval</key>
  <integer>10</integer>
</dict>
</plist>
"#,
        xml(&service.exe.display().to_string()),
        xml(&service.store.display().to_string()),
    )
}

/// The systemd user unit. It starts at login (or at boot, with lingering),
/// and again when the bridge stops with an error. Its log goes to the
/// journal unless `log_output` says otherwise.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn systemd_unit(service: &Service) -> String {
    let mut env = String::new();
    if let Some(config) = &service.config_file {
        env = format!(
            "Environment={}\n",
            systemd_quote(&format!("{CONFIG_PATH_ENV}={}", config.display()))
        );
    }
    format!(
        "[Unit]\nDescription=WhatsApp MCP bridge\n\n[Service]\nExecStart={} bridge --store {}\n{env}Restart=on-failure\nRestartSec=10\n\n[Install]\nWantedBy=default.target\n",
        systemd_quote(&service.exe.display().to_string()),
        systemd_quote(&service.store.display().to_string()),
    )
}

/// Text for a plist `<string>`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// One word on a systemd command or `Environment=` line: in double quotes,
/// with `\`, `"` escaped and `%` (a specifier) doubled.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn systemd_quote(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%");
    format!("\"{escaped}\"")
}

/// The settings the service will run with: the configuration file only,
/// since the service does not get the environment variables set here.
/// The bridge must be able to start with them.
pub fn check_config(config_file: Option<&Path>, home: Option<&Path>) -> Result<(), String> {
    let config = Config::load_from(
        |name| {
            Ok(match (name, config_file) {
                (CONFIG_PATH_ENV, Some(path)) => Some(path.display().to_string()),
                _ => None,
            })
        },
        home,
    )
    .map_err(|e| e.to_string())?;
    match config.bridge_token() {
        Ok(_) => Ok(()),
        Err(ConfigError::BridgeToken) => Err(
            "The service gets its settings from the configuration file only, and bridge_token is not set there (or shorter than 32 characters). Environment variables set here do not reach the service.".to_string(),
        ),
        Err(e) => Err(e.to_string()),
    }
}

/// `service install`: checks, writes the file and starts the service.
pub fn install(store: &Path) -> Result<(), String> {
    let store = std::path::absolute(store).map_err(|e| format!("The store folder: {e}"))?;
    if !whatsapp_mcp_bridge::is_linked(&store)? {
        return Err(format!(
            "{} is not linked yet. Run `whatsapp-mcp link --store {}` in a terminal first.",
            store.display(),
            store.display()
        ));
    }
    let config_file = std::env::var_os(CONFIG_PATH_ENV).map(PathBuf::from);
    check_config(config_file.as_deref(), std::env::home_dir().as_deref())?;
    let exe = std::env::current_exe()
        .and_then(|exe| exe.canonicalize())
        .map_err(|e| format!("Cannot find this program's own path: {e}"))?;
    let service = Service {
        exe,
        store,
        config_file,
        log_file: None,
    };
    platform::install(service)
}

/// `service uninstall`: stops the service and removes its file.
pub fn uninstall() -> Result<(), String> {
    platform::uninstall()
}

/// `service status`: whether the service is installed and running.
pub fn status() -> Result<(), String> {
    platform::status()
}

/// The API port must be free once an older service is stopped: a bridge
/// in a terminal would make the service fail and restart over and over.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn check_port_free() -> Result<(), String> {
    let address = std::net::SocketAddr::from(whatsapp_mcp_bridge::run::API_ADDRESS);
    std::net::TcpListener::bind(address).map(drop).map_err(|_| {
        format!(
            "Something already listens on {address}: stop the bridge running in a terminal first."
        )
    })
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("Failed to run {program}: {e}"))?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    if output.status.success() {
        Ok(text)
    } else {
        Err(format!(
            "{program} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn write_file(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn home() -> Result<PathBuf, String> {
    std::env::home_dir().ok_or_else(|| "No home folder".to_string())
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;

    fn plist_path() -> Result<PathBuf, String> {
        Ok(home()?
            .join("Library/LaunchAgents")
            .join(format!("{LAUNCHD_LABEL}.plist")))
    }

    fn domain() -> Result<String, String> {
        Ok(format!("gui/{}", run("id", &["-u"])?.trim()))
    }

    pub fn install(mut service: Service) -> Result<(), String> {
        let path = plist_path()?;
        let log_file = home()?.join("Library/Logs/whatsapp-mcp/bridge.log");
        service.log_file = Some(log_file.clone());
        let domain = domain()?;
        // An older version of the agent: stop it first.
        let _ = run(
            "launchctl",
            &["bootout", &format!("{domain}/{LAUNCHD_LABEL}")],
        );
        check_port_free()?;
        if let Some(dir) = log_file.parent() {
            whatsapp_mcp_store::ensure_private_dir(dir)
                .map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        write_file(&path, &launchd_plist(&service))?;
        run(
            "launchctl",
            &["bootstrap", &domain, &path.display().to_string()],
        )?;
        println!("Installed and started: {}", path.display());
        println!(
            "The bridge runs at login. Its errors go to {}, and its log where log_output says.",
            log_file.display()
        );
        Ok(())
    }

    pub fn uninstall() -> Result<(), String> {
        let path = plist_path()?;
        let _ = run(
            "launchctl",
            &["bootout", &format!("{}/{LAUNCHD_LABEL}", domain()?)],
        );
        match std::fs::remove_file(&path) {
            Ok(()) => println!("Stopped and removed: {}", path.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                println!("Not installed ({} does not exist)", path.display())
            }
            Err(e) => return Err(format!("{}: {e}", path.display())),
        }
        Ok(())
    }

    pub fn status() -> Result<(), String> {
        let path = plist_path()?;
        if !path.is_file() {
            println!("Not installed. Install it with `whatsapp-mcp service install`.");
            return Ok(());
        }
        println!("Installed: {}", path.display());
        match run(
            "launchctl",
            &["print", &format!("{}/{LAUNCHD_LABEL}", domain()?)],
        ) {
            Ok(text) => {
                for line in text.lines().map(str::trim) {
                    if line.starts_with("state = ")
                        || line.starts_with("pid = ")
                        || line.starts_with("last exit code = ")
                    {
                        println!("{line}");
                    }
                }
            }
            Err(_) => println!(
                "Not loaded: log out and in again, or run `whatsapp-mcp service install` again."
            ),
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;

    fn unit_path() -> Result<PathBuf, String> {
        Ok(home()?.join(".config/systemd/user").join(SYSTEMD_UNIT))
    }

    pub fn install(service: Service) -> Result<(), String> {
        let path = unit_path()?;
        let _ = run("systemctl", &["--user", "stop", SYSTEMD_UNIT]);
        check_port_free()?;
        write_file(&path, &systemd_unit(&service))?;
        run("systemctl", &["--user", "daemon-reload"])?;
        run("systemctl", &["--user", "enable", "--now", SYSTEMD_UNIT])?;
        println!("Installed and started: {}", path.display());
        println!(
            "The bridge runs while you are logged in; `loginctl enable-linger` keeps it running without. Its log: journalctl --user -u {SYSTEMD_UNIT}, unless log_output says otherwise."
        );
        Ok(())
    }

    pub fn uninstall() -> Result<(), String> {
        let path = unit_path()?;
        if !path.is_file() {
            println!("Not installed ({} does not exist)", path.display());
            return Ok(());
        }
        let _ = run("systemctl", &["--user", "disable", "--now", SYSTEMD_UNIT]);
        std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        run("systemctl", &["--user", "daemon-reload"])?;
        println!("Stopped and removed: {}", path.display());
        Ok(())
    }

    pub fn status() -> Result<(), String> {
        let path = unit_path()?;
        if !path.is_file() {
            println!("Not installed. Install it with `whatsapp-mcp service install`.");
            return Ok(());
        }
        println!("Installed: {}", path.display());
        // is-active and is-enabled exit non-zero for "inactive" and
        // "disabled"; their answer is on stdout either way.
        for what in ["is-active", "is-enabled"] {
            let answer = Command::new("systemctl")
                .args(["--user", what, SYSTEMD_UNIT])
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_else(|e| format!("unknown ({e})"));
            println!("{what}: {answer}");
        }
        Ok(())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod platform {
    use super::*;

    const NOT_HERE: &str = "`whatsapp-mcp service` is for macOS and Linux. On Windows, run the bridge as a scheduled task: see the README.";

    pub fn install(_service: Service) -> Result<(), String> {
        Err(NOT_HERE.to_string())
    }

    pub fn uninstall() -> Result<(), String> {
        Err(NOT_HERE.to_string())
    }

    pub fn status() -> Result<(), String> {
        Err(NOT_HERE.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> Service {
        Service {
            exe: PathBuf::from("/Users/me/bin/whatsapp-mcp"),
            store: PathBuf::from("/Users/me/Whats & <App> \"store\""),
            config_file: None,
            log_file: Some(PathBuf::from(
                "/Users/me/Library/Logs/whatsapp-mcp/bridge.log",
            )),
        }
    }

    #[test]
    fn plist_runs_the_bridge_with_quoted_paths() {
        let plist = launchd_plist(&service());
        assert!(plist.contains(&format!("<string>{LAUNCHD_LABEL}</string>")));
        assert!(plist.contains(
            "    <string>/Users/me/bin/whatsapp-mcp</string>\n    <string>bridge</string>\n    <string>--store</string>\n    <string>/Users/me/Whats &amp; &lt;App&gt; &quot;store&quot;</string>\n"
        ));
        assert!(plist.contains("<key>StandardErrorPath</key>\n  <string>/Users/me/Library/Logs/whatsapp-mcp/bridge.log</string>"));
        assert!(plist.contains("<key>SuccessfulExit</key>\n    <false/>"));
        assert!(!plist.contains("EnvironmentVariables"));

        let with_config = Service {
            config_file: Some(PathBuf::from("/etc/wa.yaml")),
            ..service()
        };
        assert!(launchd_plist(&with_config).contains(
            "<key>EnvironmentVariables</key>\n  <dict>\n    <key>WHATSAPP_MCP_CONFIG</key>\n    <string>/etc/wa.yaml</string>\n  </dict>\n"
        ));
    }

    #[test]
    fn unit_runs_the_bridge_with_quoted_paths() {
        let unit = systemd_unit(&Service {
            exe: PathBuf::from("/home/me/bin/whatsapp-mcp"),
            store: PathBuf::from("/home/me/my \"100%\" store\\x"),
            config_file: Some(PathBuf::from("/home/me/wa config.yaml")),
            log_file: None,
        });
        assert_eq!(
            unit,
            "[Unit]\nDescription=WhatsApp MCP bridge\n\n[Service]\nExecStart=\"/home/me/bin/whatsapp-mcp\" bridge --store \"/home/me/my \\\"100%%\\\" store\\\\x\"\nEnvironment=\"WHATSAPP_MCP_CONFIG=/home/me/wa config.yaml\"\nRestart=on-failure\nRestartSec=10\n\n[Install]\nWantedBy=default.target\n"
        );
    }

    #[test]
    fn the_service_needs_the_token_in_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config.yaml");
        std::fs::write(&file, "media_dir: /x\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let err = check_config(Some(&file), None).unwrap_err();
        assert!(err.contains("bridge_token is not set there"), "{err}");

        std::fs::write(&file, format!("bridge_token: {}\n", "t".repeat(32))).unwrap();
        check_config(Some(&file), None).unwrap();
    }
}
