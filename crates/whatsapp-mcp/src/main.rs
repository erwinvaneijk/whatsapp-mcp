//! `whatsapp-mcp`: the bridge and the MCP server in one binary.
//!
//! `serve` is the MCP server; `bridge` keeps the WhatsApp connection, stores
//! messages, sends and serves the local API for `serve`.

mod service;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use whatsapp_mcp_config::{Config, Setting, Source};
use whatsapp_mcp_logging::Program;
use whatsapp_mcp_store::{NamesSource, Store};

#[derive(Debug, Parser)]
#[command(version, about = "WhatsApp MCP server and bridge")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Keep the WhatsApp connection, store messages and serve the local API.
    Bridge {
        /// The store folder: messages.db, and the WhatsApp session in
        /// session.db. Created when missing. [default: store_dir in the
        /// configuration file, else ~/.local/share/whatsapp-mcp/store]
        #[arg(long, value_name = "DIR")]
        store: Option<PathBuf>,
    },
    /// Link WhatsApp: show a QR or pairing code, store the history sync, then
    /// stop. Run it in a terminal, before `service install`.
    #[command(visible_alias = "login")]
    Link {
        /// The store folder, as for `bridge`.
        #[arg(long, value_name = "DIR")]
        store: Option<PathBuf>,
    },
    /// Run the bridge as a user service (macOS: launchd, Linux: systemd).
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// Serve the MCP tools over stdio, for an MCP client such as Claude Desktop.
    Serve {
        /// The bridge's store folder, with messages.db. [default: store_dir
        /// in the configuration file, else ~/.local/share/whatsapp-mcp/store]
        #[arg(long, value_name = "DIR")]
        store: Option<PathBuf>,
        /// The bridge's local API. [default: from bridge_address in the
        /// configuration file, else http://127.0.0.1:8080/api]
        #[arg(long, value_name = "URL")]
        bridge_url: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum ServiceAction {
    /// Install and start the service, for a linked store.
    Install {
        /// The store folder, as for `bridge`.
        #[arg(long, value_name = "DIR")]
        store: Option<PathBuf>,
    },
    /// Stop the service and remove it.
    Uninstall,
    /// Say whether the service is installed and running.
    Status,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    // The service commands read the configuration file themselves, and
    // uninstall and status must work with a broken one.
    if let Command::Service { action } = cli.command {
        let result = match action {
            ServiceAction::Install { store } => whatsapp_mcp_config::Config::load()
                .map_err(|e| e.to_string())
                .and_then(|config| {
                    store_folder(
                        store,
                        &config,
                        default_store(),
                        Path::new(OLD_DEFAULT_STORE),
                    )
                })
                .and_then(|found| {
                    if let Some(warning) = &found.warning {
                        eprintln!("whatsapp-mcp service: {warning}");
                    }
                    service::install(&found.path)
                }),
            ServiceAction::Uninstall => service::uninstall(),
            ServiceAction::Status => service::status(),
        };
        return match result {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("whatsapp-mcp service: {e}");
                ExitCode::FAILURE
            }
        };
    }
    // Errors on stderr: for serve, stdout carries the MCP protocol.
    let config = match whatsapp_mcp_config::Config::load() {
        Ok(config) => config,
        Err(e) => {
            eprintln!("whatsapp-mcp: {e}");
            return ExitCode::FAILURE;
        }
    };
    let program = match cli.command {
        Command::Serve { .. } => Program::Serve,
        _ => Program::Bridge,
    };
    // Before anything is logged; a bad log setting stops here, on stderr.
    if let Err(e) = whatsapp_mcp_logging::init(program, &config) {
        eprintln!("whatsapp-mcp {}: {e}", program.name());
        return ExitCode::FAILURE;
    }
    for warning in config.warnings() {
        log::warn!("{warning}");
    }
    let link_only = matches!(cli.command, Command::Link { .. });
    let resolve =
        |flag| match store_folder(flag, &config, default_store(), Path::new(OLD_DEFAULT_STORE)) {
            Ok(found) => {
                if let Some(warning) = &found.warning {
                    log::warn!("{warning}");
                }
                Ok((found.path, found.source))
            }
            Err(e) => {
                log::error!("{e}");
                Err(ExitCode::FAILURE)
            }
        };
    match cli.command {
        Command::Serve { store, bridge_url } => {
            let (store, source) = match resolve(store) {
                Ok(found) => found,
                Err(code) => return code,
            };
            let (bridge_url, url_source) = match bridge_url_from(bridge_url, &config) {
                Ok(found) => found,
                Err(e) => {
                    log::error!("{e}");
                    return ExitCode::FAILURE;
                }
            };
            for line in serve_startup_lines(&config, &store, &source, &bridge_url, &url_source) {
                log::info!("{line}");
            }
            let options = whatsapp_mcp_server::ServeOptions {
                store_dir: store,
                bridge_url,
            };
            match whatsapp_mcp_server::serve_stdio(&config, options) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    log::error!("{e}");
                    ExitCode::FAILURE
                }
            }
        }
        Command::Bridge { store } | Command::Link { store } => {
            let (store, source) = match resolve(store) {
                Ok(found) => found,
                Err(code) => return code,
            };
            let options = whatsapp_mcp_bridge::Options {
                store_dir: store,
                store_source: source,
                link_only,
            };
            match whatsapp_mcp_bridge::run_blocking(&config, options) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    log::error!("{e}");
                    ExitCode::FAILURE
                }
            }
        }
        Command::Service { .. } => unreachable!("handled above"),
    }
}

/// The store folder before 1.2, relative to the current folder. Still used
/// when it holds a store and the new default does not (issue #74).
const OLD_DEFAULT_STORE: &str = "whatsapp-bridge/store";

/// The store folder, where it comes from (for the startup log), and a
/// warning when it is the old default.
#[derive(Debug, PartialEq, Eq)]
struct StoreFolder {
    path: PathBuf,
    source: String,
    warning: Option<String>,
}

/// The default store folder: in the user's data folder, so it does not
/// depend on the folder a program is started in. `None` without a home
/// folder (or `LOCALAPPDATA` on Windows).
fn default_store() -> Option<PathBuf> {
    let var = |name| std::env::var_os(name).map(PathBuf::from);
    default_store_from(
        cfg!(windows),
        var("LOCALAPPDATA"),
        var("XDG_DATA_HOME"),
        std::env::home_dir(),
    )
}

/// [`default_store`], with the environment given: on Windows
/// `%LOCALAPPDATA%\whatsapp-mcp\store`, else
/// `$XDG_DATA_HOME/whatsapp-mcp/store` (when absolute), else
/// `~/.local/share/whatsapp-mcp/store`.
fn default_store_from(
    windows: bool,
    local_app_data: Option<PathBuf>,
    xdg_data_home: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Option<PathBuf> {
    let data = if windows {
        local_app_data.filter(|d| d.is_absolute())?
    } else {
        match xdg_data_home.filter(|d| d.is_absolute()) {
            Some(dir) => dir,
            None => home?.join(".local").join("share"),
        }
    };
    Some(data.join("whatsapp-mcp").join("store"))
}

/// Whether a folder holds a store: messages or a WhatsApp session.
fn holds_store(dir: &Path) -> bool {
    dir.join("messages.db").is_file() || dir.join("session.db").is_file()
}

/// The store folder: `--store`, then `store_dir` (or `WHATSAPP_STORE_DIR`),
/// then the default. The old default, `whatsapp-bridge/store` in the current
/// folder, is used instead when it holds a store and the default does not,
/// with a warning, so an existing setup keeps its history.
fn store_folder(
    flag: Option<PathBuf>,
    config: &Config,
    default: Option<PathBuf>,
    old: &Path,
) -> Result<StoreFolder, String> {
    let found = |path, source: &str| StoreFolder {
        path,
        source: source.to_string(),
        warning: None,
    };
    if let Some(dir) = flag {
        return Ok(found(dir, "from --store"));
    }
    if let Some(dir) = config.store_dir().map_err(|e| e.to_string())? {
        let source = match config.source(Setting::StoreDir) {
            Source::Environment => format!("from {}", Setting::StoreDir.env()),
            _ => "from store_dir in the configuration file".to_string(),
        };
        return Ok(found(dir, &source));
    }
    match default {
        Some(new) if holds_store(&new) || !holds_store(old) => Ok(found(new, "default")),
        Some(new) => {
            let shown = std::path::absolute(old).unwrap_or_else(|_| old.to_path_buf());
            Ok(StoreFolder {
                warning: Some(format!(
                    "Using the store in the current folder, {}, because the default store folder {} is empty. Set store_dir in the configuration file to the store's absolute path, so every command finds it whatever folder it starts in.",
                    clean(&shown.display().to_string()),
                    clean(&new.display().to_string()),
                )),
                path: old.to_path_buf(),
                source: "the old default: whatsapp-bridge/store in the current folder".to_string(),
            })
        }
        // No home folder: only the old default is left.
        None => Ok(found(
            old.to_path_buf(),
            "default: no home folder, so in the current folder",
        )),
    }
}

/// The bridge's API for `serve`, and where it comes from for the startup
/// log: `--bridge-url`, else `bridge_address` (or its default).
fn bridge_url_from(flag: Option<String>, config: &Config) -> Result<(String, String), String> {
    if let Some(url) = flag {
        return Ok((url, "from --bridge-url".to_string()));
    }
    let url = config.bridge_url().map_err(|e| e.to_string())?;
    let source = match config.source(Setting::BridgeAddress) {
        Source::Environment => format!("from {}", Setting::BridgeAddress.env()),
        Source::File => "from bridge_address in the configuration file".to_string(),
        Source::Default => "default".to_string(),
    };
    Ok((url, source))
}

/// What `serve` logs on start, on stderr: the version, the settings and
/// where each comes from, the store, where names come from and the bridge.
/// A missing `messages.db` is said here: serving goes on, as the bridge may
/// create it later, but every tool would fail until then.
fn serve_startup_lines(
    config: &Config,
    store: &Path,
    source: &str,
    bridge_url: &str,
    url_source: &str,
) -> Vec<String> {
    let mut lines = vec![format!("whatsapp-mcp {} serve", env!("CARGO_PKG_VERSION"))];
    lines.extend(config.startup_lines(&[
        Setting::BridgeToken,
        Setting::FullControl,
        Setting::MediaDir,
        Setting::SendAllowlist,
        Setting::LogContent,
        Setting::LogOutput,
        Setting::LogFormat,
        Setting::LogLevel,
    ]));
    let shown = clean(
        &std::path::absolute(store)
            .unwrap_or_else(|_| store.to_path_buf())
            .display()
            .to_string(),
    );
    if store.join("messages.db").is_file() {
        lines.push(format!("Store: {shown} ({source}; messages.db found)"));
    } else {
        lines.push(format!(
            "Store: {shown} ({source}; no messages.db: start the bridge once, and set store_dir, or --store, to its store folder)"
        ));
    }
    lines.push(
        match Store::new(store).names_source() {
            NamesSource::MessagesDb => "Names: from messages.db (contacts and lid_map)",
            NamesSource::WhatsappDb => "Names: from whatsapp.db (the Go bridge)",
            NamesSource::None => "Names: none found; chats and senders show as numbers",
        }
        .to_string(),
    );
    lines.push(format!("Bridge: {} ({url_source})", clean(bridge_url)));
    lines
}

/// Removes control characters from text that comes from the command line.
fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn command_line_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn commands_parse() {
        for args in [
            &["whatsapp-mcp", "bridge"][..],
            &["whatsapp-mcp", "bridge", "--store", "/x"],
            &["whatsapp-mcp", "serve"],
            &[
                "whatsapp-mcp",
                "serve",
                "--store",
                "/x",
                "--bridge-url",
                "http://h/api",
            ],
        ] {
            assert!(Cli::try_parse_from(args).is_ok(), "{args:?}");
        }
        assert!(Cli::try_parse_from(["whatsapp-mcp"]).is_err());
        assert!(Cli::try_parse_from(["whatsapp-mcp", "unknown"]).is_err());
    }

    #[test]
    fn serve_defaults() {
        let Cli {
            command: Command::Serve { store, bridge_url },
        } = Cli::try_parse_from(["whatsapp-mcp", "serve"]).unwrap()
        else {
            panic!("not serve");
        };
        assert_eq!(store, None, "store_dir or the default decide");
        assert_eq!(bridge_url, None, "bridge_address or the default decide");
    }

    #[test]
    fn serve_says_what_it_runs_with() {
        let token = "s3cr3t-token-that-is-long-enough-1234";
        let config = Config::load_from(
            |name| {
                Ok(match name {
                    "WHATSAPP_BRIDGE_TOKEN" => Some(token.to_string()),
                    "WHATSAPP_MCP_READ_ONLY" => Some("1".to_string()),
                    _ => None,
                })
            },
            None,
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();

        let lines = serve_startup_lines(
            &config,
            dir.path(),
            "default",
            "http://127.0.0.1:8080/api",
            "default",
        );
        assert_eq!(
            lines[0],
            format!("whatsapp-mcp {} serve", env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(lines[1], "Configuration file: none");
        assert!(lines.contains(&"  bridge_token: set (from WHATSAPP_BRIDGE_TOKEN)".to_string()));
        assert!(
            lines.contains(
                &"  full_control: off (read-only: nothing is sent) (from WHATSAPP_MCP_READ_ONLY)"
                    .to_string()
            )
        );
        let store = lines.iter().find(|l| l.starts_with("Store: ")).unwrap();
        assert!(store.contains("no messages.db"), "{store}");
        assert!(
            lines.contains(&"Names: none found; chats and senders show as numbers".to_string())
        );
        assert_eq!(
            lines.last().unwrap(),
            "Bridge: http://127.0.0.1:8080/api (default)"
        );
        assert!(!lines.join("\n").contains("s3cr3t"));

        whatsapp_mcp_store::Writer::open(dir.path()).unwrap();
        let lines = serve_startup_lines(
            &config,
            dir.path(),
            "default",
            "http://127.0.0.1:8080/api",
            "default",
        );
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("Store: ") && l.ends_with("messages.db found)"))
        );
        assert!(lines.contains(&"Names: from messages.db (contacts and lid_map)".to_string()));
    }

    #[test]
    fn store_folder_from_flag_setting_or_default() {
        let home = tempfile::tempdir().unwrap();
        let load = |value: Option<&str>| {
            Config::load_from(
                |name| {
                    Ok((name == "WHATSAPP_STORE_DIR")
                        .then(|| value.map(str::to_string))
                        .flatten())
                },
                Some(home.path()),
            )
            .unwrap()
        };
        let default = home.path().join("data/whatsapp-mcp/store");
        let old = home.path().join("cwd/whatsapp-bridge/store");
        let none = load(None);
        let found = store_folder(None, &none, Some(default.clone()), &old).unwrap();
        assert_eq!(
            (found.path, found.source.as_str()),
            (default.clone(), "default")
        );
        assert_eq!(found.warning, None);
        let set = load(Some("~/wa/store"));
        let found = store_folder(None, &set, Some(default.clone()), &old).unwrap();
        assert_eq!(
            (found.path, found.source.as_str()),
            (home.path().join("wa/store"), "from WHATSAPP_STORE_DIR")
        );
        let found =
            store_folder(Some(PathBuf::from("/x")), &set, Some(default.clone()), &old).unwrap();
        assert_eq!(
            (found.path, found.source.as_str()),
            (PathBuf::from("/x"), "from --store")
        );
        let err = store_folder(None, &load(Some("relative/store")), None, &old).unwrap_err();
        assert!(err.contains("use an absolute path"), "{err}");
        // No home folder: the old default.
        let found = store_folder(None, &none, None, &old).unwrap();
        assert_eq!(found.path, old);
        assert_eq!(found.warning, None);

        // Issue #74: an existing store in the old place is still found, with
        // a warning, while the new default is empty.
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("messages.db"), b"").unwrap();
        let found = store_folder(None, &none, Some(default.clone()), &old).unwrap();
        assert_eq!(found.path, old);
        let warning = found.warning.unwrap();
        assert!(warning.contains("Set store_dir"), "{warning}");
        // Once the new default holds a store, it wins.
        std::fs::create_dir_all(&default).unwrap();
        std::fs::write(default.join("session.db"), b"").unwrap();
        let found = store_folder(None, &none, Some(default.clone()), &old).unwrap();
        assert_eq!((found.path, found.warning), (default, None));
    }

    #[test]
    fn default_store_is_in_the_data_folder() {
        let home = Some(PathBuf::from("/home/me"));
        assert_eq!(
            default_store_from(false, None, None, home.clone()),
            Some(PathBuf::from("/home/me/.local/share/whatsapp-mcp/store"))
        );
        assert_eq!(
            default_store_from(false, None, Some(PathBuf::from("/data")), home.clone()),
            Some(PathBuf::from("/data/whatsapp-mcp/store"))
        );
        // A relative XDG_DATA_HOME is ignored, as the XDG spec says.
        assert_eq!(
            default_store_from(false, None, Some(PathBuf::from("data")), home.clone()),
            Some(PathBuf::from("/home/me/.local/share/whatsapp-mcp/store"))
        );
        assert_eq!(default_store_from(false, None, None, None), None);
        #[cfg(windows)]
        assert_eq!(
            default_store_from(
                true,
                Some(PathBuf::from(r"C:\Users\me\AppData\Local")),
                None,
                home
            ),
            Some(PathBuf::from(
                r"C:\Users\me\AppData\Local\whatsapp-mcp\store"
            ))
        );
        assert_eq!(default_store_from(true, None, None, None), None);
    }

    #[test]
    fn bridge_url_from_flag_setting_or_default() {
        let load = |value: Option<&str>| {
            Config::load_from(
                |name| {
                    Ok((name == "WHATSAPP_BRIDGE_ADDRESS")
                        .then(|| value.map(str::to_string))
                        .flatten())
                },
                None,
            )
            .unwrap()
        };
        assert_eq!(
            bridge_url_from(None, &load(None)).unwrap(),
            (
                "http://127.0.0.1:8080/api".to_string(),
                "default".to_string()
            )
        );
        let set = load(Some("9090"));
        assert_eq!(
            bridge_url_from(None, &set).unwrap(),
            (
                "http://127.0.0.1:9090/api".to_string(),
                "from WHATSAPP_BRIDGE_ADDRESS".to_string()
            )
        );
        assert_eq!(
            bridge_url_from(Some("http://h/api".to_string()), &set).unwrap(),
            ("http://h/api".to_string(), "from --bridge-url".to_string())
        );
        let err = bridge_url_from(None, &load(Some("0.0.0.0:80"))).unwrap_err();
        assert!(err.contains("only a loopback address"), "{err}");
    }
}
