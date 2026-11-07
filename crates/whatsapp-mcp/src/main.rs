//! `whatsapp-mcp`: the bridge and the MCP server in one binary.
//!
//! `serve` is the MCP server; `bridge` keeps the WhatsApp connection, stores
//! messages, sends and serves the local API (phase 5 of
//! `docs/rust-1.0/plan.md`; until it is done the Go bridge is still the one
//! for daily use).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use whatsapp_mcp_config::{Config, Setting};
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
        /// session.db. Created when missing.
        #[arg(long, value_name = "DIR", default_value = "whatsapp-bridge/store")]
        store: PathBuf,
    },
    /// Serve the MCP tools over stdio, for an MCP client such as Claude Desktop.
    Serve {
        /// The bridge's store folder, with messages.db and whatsapp.db.
        #[arg(long, value_name = "DIR", default_value = "whatsapp-bridge/store")]
        store: PathBuf,
        /// The bridge's local API.
        #[arg(long, value_name = "URL", default_value = "http://127.0.0.1:8080/api")]
        bridge_url: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    // Errors on stderr: for serve, stdout carries the MCP protocol.
    let config = match whatsapp_mcp_config::Config::load() {
        Ok(config) => config,
        Err(e) => {
            eprintln!("whatsapp-mcp: {e}");
            return ExitCode::FAILURE;
        }
    };
    match cli.command {
        Command::Bridge { store } => {
            let options = whatsapp_mcp_bridge::Options { store_dir: store };
            match whatsapp_mcp_bridge::run_blocking(&config, options) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("whatsapp-mcp bridge: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Command::Serve { store, bridge_url } => {
            for line in serve_startup_lines(&config, &store, &bridge_url) {
                eprintln!("{line}");
            }
            let options = whatsapp_mcp_server::ServeOptions {
                store_dir: store,
                bridge_url,
            };
            match whatsapp_mcp_server::serve_stdio(&config, options) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("whatsapp-mcp: {e}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}

/// What `serve` logs on start, on stderr: the version, the settings and
/// where each comes from, the store, where names come from and the bridge.
/// A missing `messages.db` is said here: serving goes on, as the bridge may
/// create it later, but every tool would fail until then.
fn serve_startup_lines(config: &Config, store: &Path, bridge_url: &str) -> Vec<String> {
    let mut lines = vec![format!("whatsapp-mcp {} serve", env!("CARGO_PKG_VERSION"))];
    lines.extend(config.startup_lines(&[
        Setting::BridgeToken,
        Setting::ReadOnly,
        Setting::MediaDir,
        Setting::SendAllowlist,
        Setting::LogContent,
    ]));
    let shown = clean(
        &std::path::absolute(store)
            .unwrap_or_else(|_| store.to_path_buf())
            .display()
            .to_string(),
    );
    if store.join("messages.db").is_file() {
        lines.push(format!("Store: {shown} (messages.db found)"));
    } else {
        lines.push(format!(
            "Store: {shown} (no messages.db: start the bridge once, and point --store at its store folder, as an absolute path)"
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
    lines.push(format!("Bridge: {}", clean(bridge_url)));
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
        assert_eq!(store, PathBuf::from("whatsapp-bridge/store"));
        assert_eq!(bridge_url, "http://127.0.0.1:8080/api");
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

        let lines = serve_startup_lines(&config, dir.path(), "http://127.0.0.1:8080/api");
        assert_eq!(
            lines[0],
            format!("whatsapp-mcp {} serve", env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(lines[1], "Configuration file: none");
        assert!(lines.contains(&"  bridge_token: set (from WHATSAPP_BRIDGE_TOKEN)".to_string()));
        assert!(lines.contains(&"  read_only: on (from WHATSAPP_MCP_READ_ONLY)".to_string()));
        let store = lines.iter().find(|l| l.starts_with("Store: ")).unwrap();
        assert!(store.contains("no messages.db"), "{store}");
        assert!(
            lines.contains(&"Names: none found; chats and senders show as numbers".to_string())
        );
        assert_eq!(lines.last().unwrap(), "Bridge: http://127.0.0.1:8080/api");
        assert!(!lines.join("\n").contains("s3cr3t"));

        whatsapp_mcp_store::Writer::open(dir.path()).unwrap();
        let lines = serve_startup_lines(&config, dir.path(), "http://127.0.0.1:8080/api");
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("Store: ") && l.ends_with("(messages.db found)"))
        );
        assert!(lines.contains(&"Names: from messages.db (contacts and lid_map)".to_string()));
    }
}
