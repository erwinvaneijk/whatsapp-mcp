//! `whatsapp-mcp`: the bridge and the MCP server in one binary.
//!
//! Both commands are placeholders until their phase of the plan
//! (`docs/rust-1.0/plan.md`); until then, use the Go bridge and the Python
//! server.

use std::process::ExitCode;

use clap::{Parser, Subcommand};

// Linked now so the workspace builds as one program; used from phase 3 on.
use whatsapp_mcp_bridge as _;
use whatsapp_mcp_config as _;
use whatsapp_mcp_server as _;
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
    Bridge,
    /// Serve the MCP tools over stdio, for an MCP client such as Claude Desktop.
    Serve,
}

/// Exit code for a command that does not exist yet.
const NOT_IMPLEMENTED: u8 = 2;

fn main() -> ExitCode {
    let cli = Cli::parse();
    // stderr only: once `serve` works, stdout carries the MCP protocol.
    let (what, phase, instead) = match cli.command {
        Command::Bridge => ("bridge", 5, "the Go bridge in whatsapp-bridge/"),
        Command::Serve => ("serve", 4, "the Python server in whatsapp-mcp-server/"),
    };
    eprintln!(
        "whatsapp-mcp {what}: not implemented yet (phase {phase} of docs/rust-1.0/plan.md). Use {instead}."
    );
    ExitCode::from(NOT_IMPLEMENTED)
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
    fn both_commands_parse() {
        for name in ["bridge", "serve"] {
            assert!(
                Cli::try_parse_from(["whatsapp-mcp", name]).is_ok(),
                "{name}"
            );
        }
        assert!(Cli::try_parse_from(["whatsapp-mcp"]).is_err());
        assert!(Cli::try_parse_from(["whatsapp-mcp", "unknown"]).is_err());
    }
}
