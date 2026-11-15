# whatsapp-mcp plugin for Claude Code

Search and read your WhatsApp messages from Claude Code, and, if you turn on full control, send them. The messages stay on your computer: a bridge keeps the connection with WhatsApp and stores them there, and Claude only sees what a tool returns.

Read-only by default. Anyone who can send you a WhatsApp message controls text that Claude reads, and could try to make it send your conversations to them (prompt injection). Turn on **Full control** in the plugin's settings only if you need Claude to send.

## What it runs

- **The MCP server**, `whatsapp-mcp serve`, started by Claude Code for each session through [`scripts/launch.sh`](scripts/launch.sh).
- **Nothing else.** The bridge, `whatsapp-mcp bridge`, must run all the time, so the plugin doesn't start it. You install it as a service once; see [Setup](#setup).

## What it downloads

On first start, `launch.sh` downloads the `whatsapp-mcp` program for your platform from this version's [GitHub release](https://github.com/erwinvaneijk/whatsapp-mcp/releases/tag/v1.0.0): macOS (arm64, x86_64), Linux (x86_64, arm64) or Windows (x86_64, beta). It checks the file against the checksums pinned in [`scripts/SHA256SUMS`](scripts/SHA256SUMS), and refuses to run a file that doesn't match. Nothing else is fetched.

## What it stores

- **The program:** in the plugin's data folder (`~/.claude/plugins/data/…/bin/`), kept across plugin updates.
- **Your messages and the WhatsApp session:** in the store folder you choose (`store_dir`), written by the bridge, not by the plugin.
- **Settings:** in the whatsapp-mcp configuration file (`~/.config/whatsapp-mcp/config.yaml`), including the bridge token. The plugin's own settings are only **Full control** and the path of that file.

## Setup

1. Write the configuration file, with a bridge token and `store_dir`: see [Configuration](https://github.com/erwinvaneijk/whatsapp-mcp#configuration).
2. Link WhatsApp in a terminal, with the program the plugin downloaded: `whatsapp-mcp link`.
3. Run the bridge as a service: `whatsapp-mcp service install` (macOS, Linux).
4. Start a new Claude Code session and ask about your messages.

Or ask Claude to set it up: the plugin's `whatsapp-setup` skill walks through these steps, and does what it can for you. The full steps are also in the [main README](https://github.com/erwinvaneijk/whatsapp-mcp#installation).

## Settings

| Setting | Default | Meaning |
|---|---|---|
| Full control (sending) | off | Offer the tools that send messages and reactions. The bridge also needs `full_control: yes` in the configuration file before it sends. |
| Configuration file | `~/.config/whatsapp-mcp/config.yaml` | The file shared with the bridge. |

## Removing it

Uninstall the plugin, then stop the bridge with `whatsapp-mcp service uninstall`. Your store folder (messages and session) stays until you delete it; also remove the linked device on your phone under WhatsApp > Linked devices.
