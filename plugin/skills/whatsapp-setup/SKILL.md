---
name: whatsapp-setup
description: Set up, check, upgrade or remove the WhatsApp bridge that the whatsapp-mcp plugin reads from. Use when the user wants to start using WhatsApp in Claude Code, when the WhatsApp tools fail (no messages.db, bridge not running, connection refused, unauthorized), when they want Claude to be able to send, or when they want to remove it.
---

# Set up the WhatsApp bridge

The plugin starts the MCP server (`whatsapp-mcp serve`). It reads messages that the **bridge** (`whatsapp-mcp bridge`) stores, and sends through it. The bridge must run all the time, so it runs as a service of the user's own, outside Claude Code. Your job is to get the user from nothing to a linked, running bridge, one step at a time, checking each step before the next.

The program the plugin downloaded is:

```
${CLAUDE_PLUGIN_DATA}/bin/whatsapp-mcp
```

(`whatsapp-mcp.exe` on Windows). Call it `$BIN` below, and always use this full path: there may be another `whatsapp-mcp` on the PATH, at another version. If it does not exist yet, get it (this downloads and checks the release binary):

```bash
CLAUDE_PLUGIN_DATA="${CLAUDE_PLUGIN_DATA}" bash "${CLAUDE_PLUGIN_ROOT}/scripts/launch.sh" --version
```

## Rules

- **Never show the bridge token.** Don't print the configuration file whole; when you read it, hide `bridge_token`'s value. Don't put the token in a command line, a message or a file other than the configuration file.
- **Ask before changing the user's system:** writing the configuration file, installing or removing the service, deleting anything.
- **Linking needs the user's own terminal.** It shows a QR code and waits for the phone; your shell can't show it. Give the user the exact command to run in a terminal window, and wait for them.
- **Sending is off by default, on purpose.** Anyone who can message the user controls text you read, and could try to make you send their conversations away (prompt injection). Only turn on full control when the user asks for it, after saying so.

## 1. Check what is there

Run these and summarize what you find:

- `"$BIN" --version`
- Whether `~/.config/whatsapp-mcp/config.yaml` exists, its permissions, and its setting names (not the token's value). The plugin's **Configuration file** setting may name another file; if `WHATSAPP_MCP_CONFIG` is set in your environment, that is the one.
- `"$BIN" service status` (macOS, Linux).
- Whether something listens on `127.0.0.1:8080` (`lsof -nP -iTCP:8080 -sTCP:LISTEN` on macOS and Linux). That is a bridge already running, possibly the old Go bridge or one in a terminal.

If an older setup is there (a `whatsapp-bridge/store` folder, the Go bridge, a bridge started by hand), follow the [upgrade guide](https://github.com/erwinvaneijk/whatsapp-mcp/blob/main/docs/upgrading-to-1.0.md) with the user instead: it keeps their message history.

Then do only the steps that are missing.

## 2. Configuration file

Suggest trying a separate WhatsApp test account first: whatsapp-mcp is an unofficial client, and WhatsApp may restrict accounts that use one.

Ask the user, then create `~/.config/whatsapp-mcp/config.yaml`, readable by them only. Generate the token in the same command that writes the file, so it never appears in the output:

```bash
mkdir -p ~/.config/whatsapp-mcp
umask 077
cat > ~/.config/whatsapp-mcp/config.yaml <<EOF
# whatsapp-mcp: read by the bridge and the MCP server.
bridge_token: "$(head -c 32 /dev/urandom | base64)"
store_dir: ~/.local/share/whatsapp-mcp/store
full_control: no
EOF
chmod 600 ~/.config/whatsapp-mcp/config.yaml
```

Then ask about the optional settings and add the ones the user wants:

- `media_dir`: the only folder files may be sent from. Without it, sending files is off.
- `send_allowlist`: the chats the bridge may send to (phone numbers with country code, or group JIDs), as a YAML list. Recommend it if they turn on full control.
- `pair_phone`: their phone number with country code (such as `31612345678`), to link with an 8-character code instead of a QR code.

If the file already exists, keep it; only add `store_dir` if it is missing (ask where their store folder is).

## 3. Link WhatsApp

Ask the user to run this in a terminal window, and to tell you when it has finished:

```
"<the full path of $BIN>" link
```

It shows a QR code: on the phone, WhatsApp > Linked devices > Link a device, and scan it. With `pair_phone` it prints a code to enter under *Link with phone number instead*. After linking, WhatsApp sends the recent history; `link` stores it and stops by itself (when nothing new arrives for a minute).

If it says the store is already linked, go on.

## 4. Run the bridge as a service

On macOS and Linux, after asking:

```bash
"$BIN" service install
"$BIN" service status
```

It refuses an unlinked store, a configuration file without the token, and a busy port 8080; fix what it says. Mention:

- macOS: the log is in `~/Library/Logs/whatsapp-mcp/bridge.log` (or where `log_output` says).
- Linux: `journalctl --user -u whatsapp-mcp-bridge`; `loginctl enable-linger` keeps it running when they are not logged in.

The service runs `$BIN`, the file in the plugin's data folder, so plugin updates update the bridge on its next restart.

On Windows there is no `service` command. Give the user the scheduled task from the [README](https://github.com/erwinvaneijk/whatsapp-mcp#windows), with `$BIN` as the program, and suggest `log_output` set to a file.

## 5. Check that it works

- `"$BIN" service status` says it is running.
- The store folder has `messages.db`.
- The MCP server started with this session may have read an older configuration. Ask the user to run `/mcp` and reconnect `whatsapp` (or start a new session), then call `list_chats` with a small limit. Chats with names mean it works.

If a tool fails: "Unauthorized" means the server and the bridge have different tokens (different configuration files); "connection refused" means the bridge isn't running; "no messages.db" means `store_dir` differs between them.

## Sending (full control)

Only when the user asks. Two switches, both needed:

1. In the configuration file: `full_control: yes` (the bridge refuses to send without it). Then restart the bridge: `"$BIN" service uninstall` and `"$BIN" service install`.
2. In Claude Code: `/plugin`, the whatsapp-mcp plugin's settings, **Full control (sending)** on. Then reconnect the server with `/mcp`.

Suggest a `send_allowlist` with it.

## Removing it

After asking: `"$BIN" service uninstall`, then uninstall the plugin. The store folder (messages, session, media) and the configuration file stay until the user deletes them; offer to, and remind them to remove the linked device on the phone under WhatsApp > Linked devices.
