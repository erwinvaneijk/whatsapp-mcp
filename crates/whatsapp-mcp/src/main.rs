//! `whatsapp-mcp`: the bridge and the MCP server in one binary.
//!
//! `serve` is the MCP server; `bridge` keeps the WhatsApp connection, stores
//! messages, sends and serves the local API (phase 5 of
//! `docs/rust-1.0/plan.md`; until it is done the Go bridge is still the one
//! for daily use).

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

// Linked so the workspace builds as one program.
use whatsapp_mcp_store as _;

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
            // Keep serving: the bridge may create it later. But say so now,
            // instead of leaving every tool to fail with a database error.
            if !store.join("messages.db").is_file() {
                eprintln!(
                    "whatsapp-mcp: no messages.db in {}. Start the bridge once, and point --store at its store folder (an absolute path).",
                    store.display()
                );
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
}
