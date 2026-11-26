# WhatsApp MCP Server

This is a Model Context Protocol (MCP) server for WhatsApp.

With this you can search and read your personal Whatsapp messages (including images, videos, documents, and audio messages), search your contacts and send messages to either individuals or groups. You can also send media files including images, videos, documents, and audio messages.

It connects to your **personal WhatsApp account** directly via the Whatsapp web multidevice API (using the [whatsapp-rust](https://github.com/oxidezap/whatsapp-rust) library). All your messages are stored locally in a SQLite database and only sent to an LLM (such as Claude) when the agent accesses them through tools (which you control).

Here's an example of what you can do when it's connected to Claude.

![WhatsApp MCP](./example-use.png)

> *Caution:* as with many MCP servers, the WhatsApp MCP is subject to [the lethal trifecta](https://simonwillison.net/2025/Jun/16/the-lethal-trifecta/). This means that prompt injection could lead to private data exfiltration.
> Any message you receive is untrusted input that the model reads: someone can send you a message containing instructions (prompt injection) that try to make the agent forward your private conversations or files to them. That is why nothing is sent by default: the model can read and search, but not send. Turn sending on only if you need it; see [Full control (sending)](#full-control-sending).

## Where this comes from

This server began as [lharries/whatsapp-mcp](https://github.com/lharries/whatsapp-mcp), a Go bridge and a Python server, and is now one Rust program with the open upstream fixes merged in. [History](docs/history.md) tells how. Use the `main` branch or a [release](https://github.com/erwinvaneijk/whatsapp-mcp/releases); [Branches](docs/branches.md) explains the other branches.

## Installation

One program, `whatsapp-mcp`, does both jobs:

- `whatsapp-mcp bridge` keeps the connection with WhatsApp and stores your messages. It runs all the time, in a terminal or as a service.
- `whatsapp-mcp serve` is the MCP server. Your MCP client (Claude Desktop, Cursor) starts it.

### Prerequisites

- An MCP client, such as Claude Desktop or Cursor.
- FFmpeg (_optional_), to send audio as WhatsApp voice messages. Voice messages must be `.ogg` Opus; with FFmpeg the MCP server converts other audio. Without it, `send_file` still sends audio as a plain file.
- To build from source instead of downloading: [Rust](https://rustup.rs) (`rustup`) and a C compiler, for the SQLite that is built in. That is the Xcode command line tools on macOS, `gcc` or `clang` on Linux, the Visual Studio build tools on Windows.

### Steps

1. **Get the program**

   Download `whatsapp-mcp` for your platform from the [releases page](https://github.com/erwinvaneijk/whatsapp-mcp/releases): macOS (arm64, x86_64), Linux (x86_64, arm64) or Windows (x86_64). Check it against `SHA256SUMS` and make it executable:

   ```bash
   shasum -a 256 -c SHA256SUMS --ignore-missing   # sha256sum on Linux
   mv whatsapp-mcp-aarch64-apple-darwin ~/.local/bin/whatsapp-mcp
   chmod +x ~/.local/bin/whatsapp-mcp
   ```

   Or build it:

   ```bash
   git clone https://github.com/erwinvaneijk/whatsapp-mcp.git
   cd whatsapp-mcp
   cargo build --release
   ```

   This makes `target/release/whatsapp-mcp` (`whatsapp-mcp.exe` on Windows). Build it again after pulling new changes.

2. **Write the configuration file**

   The bridge only starts with a shared secret (the bridge token, at least 32 characters), and only sends files from one folder (the media folder). See [Bridge token and media folder](#bridge-token-and-media-folder). Both `bridge` and `serve` read them from one [configuration file](#configuration):

   ```bash
   mkdir -p ~/.config/whatsapp-mcp
   curl -o ~/.config/whatsapp-mcp/config.yaml https://raw.githubusercontent.com/erwinvaneijk/whatsapp-mcp/main/config.example.yaml
   chmod 600 ~/.config/whatsapp-mcp/config.yaml
   head -c 32 /dev/urandom | base64   # paste this as bridge_token in the file
   ```

   Set `media_dir` to the folder you will send files from, or leave it empty to turn sending files off.

   `store_dir` is the store folder: it holds your messages (`messages.db`), the WhatsApp session (`session.db`) and downloaded media, and is created when missing. The example sets it to `~/.local/share/whatsapp-mcp/store`, which is also the default when `store_dir` is empty (`%LOCALAPPDATA%\whatsapp-mcp\store` on Windows; `$XDG_DATA_HOME/whatsapp-mcp/store` when that is set). Every command reads it from the file, so you don't need `--store`; `--store DIR` still overrides it for one command.

   Environment variables work too, instead of the file or on top of it; see [Configuration](#configuration).

3. **Link WhatsApp**

   In a terminal:

   ```bash
   whatsapp-mcp link
   ```

   It shows a QR code. On your phone open WhatsApp > Linked devices > Link a device, and scan it. To link with a code instead, set `pair_phone` (or `WHATSAPP_PAIR_PHONE`) to your phone number with country code, such as `31612345678`. It then prints an 8-character code; on your phone choose Link with phone number instead, and enter it.

   After linking, WhatsApp sends your recent history. `link` stores it and stops by itself once it is in (when no more arrives for a minute). `whatsapp-mcp login` does the same.

4. **Run the bridge**

   The bridge must keep running: it receives your messages and sends for the MCP server. On macOS and Linux, install it as a service that starts when you log in:

   ```bash
   whatsapp-mcp service install
   whatsapp-mcp service status
   ```

   - **macOS:** a launchd agent in `~/Library/LaunchAgents`. With the default `log_output: stderr` its log goes to `~/Library/Logs/whatsapp-mcp/bridge.log`, in a folder only you can read; or set `log_output` to a file of your own or `syslog` (see [Logs](#logs)).
   - **Linux:** a systemd user unit, `whatsapp-mcp-bridge.service`. Its log is in `journalctl --user -u whatsapp-mcp-bridge`. To keep it running when you are not logged in: `loginctl enable-linger`.
   - The service only reads the [configuration file](#configuration), not the environment variables you set in your shell; `install` checks that the bridge token is in the file. It also refuses a store that is not linked yet.
   - `whatsapp-mcp service uninstall` stops and removes it. Run `install` again after moving the program or the store.

   Or run it in a terminal: `whatsapp-mcp bridge`, and leave it open. For Windows, see [Windows](#windows).

5. **Connect your MCP client**

   Copy the JSON below. Use absolute paths: the client starts the server from a folder of its own choosing.

   ```json
   {
     "mcpServers": {
       "whatsapp": {
         "command": "/absolute/path/to/whatsapp-mcp",
         "args": ["serve"]
       }
     }
   }
   ```

   The server reads the store folder, the token and the media folder from the configuration file; without the file, add them in an `"env"` block as `WHATSAPP_STORE_DIR`, `WHATSAPP_BRIDGE_TOKEN` and `WHATSAPP_MEDIA_DIR`. If you moved the bridge to another port with [`bridge_address`](#configuration), `serve` reads that too; `"--bridge-url", "http://…/api"` in the arguments overrides it.

   For **Claude**, save this as `claude_desktop_config.json` in your Claude Desktop configuration directory at:

   ```
   ~/Library/Application Support/Claude/claude_desktop_config.json
   ```

   For **Cursor**, save this as `mcp.json` in your Cursor configuration directory at:

   ```
   ~/.cursor/mcp.json
   ```

6. **Restart Claude Desktop / Cursor**

   Open Claude Desktop and you should now see WhatsApp as an available integration.

   Or restart Cursor.

### Install as a Claude Code plugin

In Claude Code, the plugin does steps 1 and 5 for you: it downloads the program, checks it, and runs the MCP server.

```sh
claude plugin install whatsapp-mcp --marketplace erwinvaneijk/whatsapp-mcp
```

Then ask Claude to set up WhatsApp: the plugin's `whatsapp-setup` skill writes the configuration file, has you link WhatsApp, and installs the bridge as a service. See the [plugin's README](plugin/README.md) for what it downloads, runs and stores.

### Upgrading to 1.0

From 1.0 the Go bridge is replaced by `whatsapp-mcp bridge`. Your messages and configuration stay; you link WhatsApp again once. See [Upgrading to 1.0](docs/upgrading-to-1.0.md).

### Upgrading from the Python MCP server

Before version 0.9, the MCP server was a Python program run with `uv`. From 0.9 it is `whatsapp-mcp serve`, with the same tools and the same configuration. In your MCP client's configuration, replace `"command"` and `"args"` with the ones in step 5, keep the `env` block if you have one, and restart the client. Then follow [Upgrading to 1.0](docs/upgrading-to-1.0.md) for the bridge.

A few error messages are clearer than in the Python server; see [Behaviour that improves](docs/archive/rust-1.0/interface.md#5-behaviour-that-improves-in-10).

### Windows

**Windows is beta in 1.0.** The Windows build is made and tested in CI, but has not been tried with WhatsApp yet. Problems are welcome as [issues](https://github.com/erwinvaneijk/whatsapp-mcp/issues). `log_output: syslog` is not available on Windows, and the store folder's permissions work differently there; see [Data Storage](#data-storage).

`whatsapp-mcp service` is not available on Windows. Link with `whatsapp-mcp link` in a terminal, then run the bridge as a scheduled task that starts when you log in. In PowerShell, with your own paths:

```powershell
$action = New-ScheduledTaskAction -Execute "C:\Users\me\bin\whatsapp-mcp.exe" -Argument 'bridge --store "C:\Users\me\whatsapp-store"'
$trigger = New-ScheduledTaskTrigger -AtLogOn -User $env:USERNAME
$settings = New-ScheduledTaskSettingsSet -RestartCount 999 -RestartInterval (New-TimeSpan -Minutes 1) -ExecutionTimeLimit 0
Register-ScheduledTask -TaskName "whatsapp-mcp bridge" -Action $action -Trigger $trigger -Settings $settings
```

Set `log_output` to a file in the configuration file, since a scheduled task has no terminal. Remove the task with `Unregister-ScheduledTask -TaskName "whatsapp-mcp bridge"`.

## Configuration

The bridge and the MCP server read their settings from environment variables, or from one YAML file: `~/.config/whatsapp-mcp/config.yaml`, or the path in `WHATSAPP_MCP_CONFIG`. [`config.example.yaml`](config.example.yaml) lists every setting with an explanation. An environment variable that is set takes precedence over the same setting in the file.

| Setting in the file | Environment variable | Used by | Meaning |
|---|---|---|---|
| `bridge_token` | `WHATSAPP_BRIDGE_TOKEN` | both | Shared secret, at least 32 characters. Required. |
| `bridge_address` | `WHATSAPP_BRIDGE_ADDRESS` | both | Where the bridge's local API listens, and where `serve` reaches it: `127.0.0.1:8080` by default. `IP:PORT`, `localhost:PORT` or a port alone; only addresses on this computer (loopback) are accepted. |
| `store_dir` | `WHATSAPP_STORE_DIR` | both | The store folder, absolute or starting with `~/`. `--store` overrides it. Default: `~/.local/share/whatsapp-mcp/store` (`$XDG_DATA_HOME/whatsapp-mcp/store` when set; `%LOCALAPPDATA%\whatsapp-mcp\store` on Windows). |
| `media_dir` | `WHATSAPP_MEDIA_DIR` | both | The only folder files can be sent from. |
| `send_allowlist` | `WHATSAPP_SEND_ALLOWLIST` | bridge | Chats the bridge may send to; a YAML list in the file. See [Control whom the bridge is allowed to send to](#control-whom-the-bridge-is-allowed-to-send-to). |
| `full_control` | `WHATSAPP_FULL_CONTROL` | both | Allow sending; off by default. See [Full control (sending)](#full-control-sending). Replaces `read_only` (`WHATSAPP_MCP_READ_ONLY`), which still works until 2.0. |
| `log_content` | `WHATSAPP_LOG_CONTENT` | bridge | Write message content to the logs. See [Logs](#logs). |
| `pair_phone` | `WHATSAPP_PAIR_PHONE` | bridge | Link with a pairing code instead of a QR code. |
| `log_output` | `WHATSAPP_LOG_OUTPUT` | `whatsapp-mcp` | Where log lines go: `stderr` (default), `stdout` (not for `serve`), `syslog`, or a file. See [Logs](#logs). |
| `log_format` | `WHATSAPP_LOG_FORMAT` | `whatsapp-mcp` | `text` (default) or `json`. |
| `log_level` | `WHATSAPP_LOG_LEVEL` | `whatsapp-mcp` | `error`, `warn`, `info` (default) or `debug`. |

- The file holds the token, so `bridge` and `serve` refuse it when other users can read or change it (`chmod 600` fixes that). This check is skipped on Windows.
- An unknown setting stops them with the line number, so a typo cannot go unnoticed.
- Values are used exactly as written; YAML does not turn `0612345678` into another number.
- A file named in `WHATSAPP_MCP_CONFIG` must exist; the default file is optional.

## Security

Found a security problem? Please report it privately, as described in [SECURITY.md](SECURITY.md), not in a public issue.

### Bridge token and media folder

The bridge's REST API is locked down. It refuses to start unless the bridge and the MCP server share a secret, and it only sends files from one folder.

- `WHATSAPP_BRIDGE_TOKEN`: a random secret of at least 32 characters. Set the same value for the bridge and for the MCP server. Every request must carry it in an `X-Bridge-Token` header. Generate one with `head -c 32 /dev/urandom | base64`.
- `WHATSAPP_MEDIA_DIR`: the only directory `send_file` and `send_audio_message` may read from. Sending files is refused when it is unset.
- The API listens on `127.0.0.1` only (port 8080, or the one in `bridge_address`; addresses reachable from the network are refused), and only accepts `Content-Type: application/json`.
- Downloaded media filenames and chat IDs are sanitised, so a sender cannot write outside `store/`.
- Message text is stripped of control characters before it is printed to the terminal.
- The store folder (session keys, messages and media) is readable by your user only.

To call the API by hand, pass the token:

```bash
curl -s -X POST http://127.0.0.1:8080/api/download \
  -H 'Content-Type: application/json' -H "X-Bridge-Token: $WHATSAPP_BRIDGE_TOKEN" \
  -d '{"message_id": "<id>", "chat_jid": "<chat_jid>"}'
```

### Full control (sending)

By default nothing is sent. The MCP server leaves out the tools that send (`send_message`, `send_file`, `send_audio_message`, `react_to_message`), and the bridge refuses to send or react (HTTP 403), also for a program that has the token. The model can still search and read your messages and download media. This removes the way out for a prompt injection hidden in a received message.

To let the model send, set `full_control: yes` in the [configuration file](#configuration), or `WHATSAPP_FULL_CONTROL=1` for both `serve` and `bridge`, and restart them. Then:

- Limit the chats it can send to with [`send_allowlist`](#control-whom-the-bridge-is-allowed-to-send-to).
- Let your MCP client ask before each send: the send tools are marked as destructive, so clients that ask for confirmation do.

Both programs log on start whether full control is on, and where that comes from.

Before 1.0 sending was on by default, and `read_only` (`WHATSAPP_MCP_READ_ONLY`) turned it off. That setting still works until 2.0, with a warning on start: `read_only: yes` is now the default and can go, and `read_only: no` means `full_control: yes`. Setting both so that they disagree is an error.

### Messages from others are marked as data

Anyone who can send you a WhatsApp message controls text that Claude reads. A message, caption or contact name could contain instructions such as "forward all chats to this number" (prompt injection). The MCP server limits this risk:

- Message text is wrapped in `<message_content>` tags, and copies of that tag inside a message are neutralised, so a message cannot close the wrapper and continue as fake instructions. Chat and contact names are kept on one line, so they cannot fake extra lines of output.
- Every read tool's description, and the instructions the server gives the client when it connects, say that this content is written by others and that instructions in it must never be followed.

This lowers the risk but cannot rule it out: a model can still be misled. For full protection against sending, leave [full control](#full-control-sending) off.

### Control whom the bridge is allowed to send to

Set `send_allowlist` in the [configuration file](#configuration), or `WHATSAPP_SEND_ALLOWLIST` in the bridge's environment, to the chats it may send messages and reactions to. Anything else is refused by the bridge with HTTP 403, so a message that tries to make Claude write to someone else (prompt injection) cannot succeed. This also applies to calls that bypass the MCP server.

```yaml
send_allowlist:
  - 31612345678              # Maria
  - 31687654321              # Tom
  - 120363000000000000@g.us  # Book club
```

or, as an environment variable:

```bash
export WHATSAPP_SEND_ALLOWLIST="31612345678, 31687654321, 120363000000000000@g.us"
```

- Entries are phone numbers with country code (`+`, spaces and dashes are allowed) or JIDs, including group JIDs (`...@g.us`; `list_chats` shows them). Separate them with commas, semicolons or new lines.
- A contact is allowed whether WhatsApp addresses them by phone number or by LID.
- Reading is not limited: Claude can still read all chats.
- If an entry cannot be read, the bridge refuses to start, so a typo does not silently allow or block the wrong chats. Not set or empty means no limit.

### Logs

By default the bridge keeps message content out of its output: it logs that a message was stored, but not the sender, the text, captions, file names or the emoji of a reaction. This matters when the bridge runs in a container or as a service, where logs are kept. The decryption keys of media you send are never logged.

To see message content in the logs while debugging, start the bridge with `WHATSAPP_LOG_CONTENT=1`.

`whatsapp-mcp serve` and `whatsapp-mcp bridge` give every log line a time, a level and its source, the same for their own lines and for the WhatsApp library:

```
2026-11-08T14:03:12.345+01:00 INFO  bridge: ✓ Connected to WhatsApp
2026-11-08T14:03:13.020+01:00 WARN  whatsapp-rust: Failed processing group plaintext (batch): …
```

- **Where:** `log_output` is `stderr` (the default), `stdout`, `syslog` (the local syslog; not on Windows), or a file path, absolute or starting with `~/`. A file is appended to and readable by you only; rotate it with `logrotate` or `newsyslog`. `serve` refuses `stdout`, which carries the MCP protocol.
- **syslog on macOS** goes to the unified log, with subsystem `whatsapp-mcp` and `serve` or `bridge` as category: `log show --last 1h --predicate 'subsystem == "whatsapp-mcp"'`, or `log stream` with the same predicate. Errors show as faults and warnings as errors there, as macOS uses those levels.
- **Format:** `log_format: json` writes one JSON object per line, with `time`, `level`, `source` and `message`.
- **Level:** `log_level` sets the level of the programs' own lines (default `info`); the library's lines are shown from `warn`. `RUST_LOG` (such as `RUST_LOG=whatsapp_rust=debug`) goes on top, per module, for debugging; debug output is not meant for logs you share.
- **Linking:** the QR code and the pairing code always appear on the terminal (stderr), whatever `log_output` says; the log only notes that one was shown.
- Both programs start by logging the settings they run with and where each comes from (the token only as set or not set).

## Architecture Overview

`whatsapp-mcp` is one Rust program, built from [`crates/`](crates/README.md), with two commands:

1. **Bridge** (`whatsapp-mcp bridge`): connects to WhatsApp as a linked device, with [whatsapp-rust](https://github.com/oxidezap/whatsapp-rust). It stores every message that arrives in `messages.db`, and serves a local API on `127.0.0.1:8080` (or [`bridge_address`](#configuration)) to send and to download media. Before version 1.0 this was a Go program.
2. **MCP server** (`whatsapp-mcp serve`): the tools your MCP client uses. It reads `messages.db` itself, and asks the bridge to send and to download. Before version 0.9 this was a Python server.

How a request flows:

- **Reading** (searching, listing, a message's context): the client asks `serve`, which reads `messages.db` and answers. The bridge is not involved, so reading works while the bridge is stopped, with the messages stored so far.
- **Sending and downloading**: `serve` passes the request to the bridge's API, with the bridge token. The bridge checks it (full control, the send allowlist, the media folder), talks to WhatsApp, and answers.
- **Receiving**: the bridge stores new messages as they arrive; the next read sees them.

### Data Storage

Everything is in the store folder (`store_dir`; by default `~/.local/share/whatsapp-mcp/store`):

| File | What it holds |
|---|---|
| `messages.db` | Chats, messages (with a full-text index for searching), and contact names. Media is stored as details only: type, file name and the keys to download it. |
| `session.db` | The WhatsApp session: the keys of this linked device. Anyone with this file has access to your account. |
| `<chat>/<file>` | Media you downloaded, in a folder per chat. |

- The bridge restricts the store folder to your user: the folder is set to `0700` (also when it already exists), and the databases and downloaded media to `0600`.
- On Windows these permission bits do not apply; the folder inherits the access rules of its parent. Keep the store folder inside your user profile (the default is), or restrict the folder explicitly, e.g. `icacls <store folder> /inheritance:r /grant:r "%USERNAME%:(OI)(CI)F"`.

## Usage

Once connected, you can interact with your WhatsApp contacts through Claude, leveraging Claude's AI capabilities in your WhatsApp conversations.

### MCP Tools

These tools are always there. They read, and never send anything:

- **search_contacts**: find contacts by name or phone number
- **list_messages**: messages, filtered by chat, sender, text or time, with the messages around each match
- **list_chats**: chats, by last activity or by name
- **get_chat**: one chat
- **get_direct_chat_by_contact**: the direct chat with a phone number
- **get_contact_chats**: the chats a contact is in
- **get_last_interaction**: the latest message with a contact
- **get_message_context**: the messages before and after a message
- **list_media**: messages with a photo, video, audio or document, optionally in one chat or of one kind, with their file names
- **list_recent_activity**: the chats with messages after a given time, and how many, for "what did I miss?"
- **download_media**: download the photo, video, audio or document of a message, and get the file's path

These send, so they are only there with [full control](#full-control-sending) on:

- **send_message**: send a text message to a phone number or a group, optionally as a reply or with mentions
- **send_file**: send a photo, video, document or other file from the media folder
- **send_audio_message**: send audio from the media folder as a voice message (Opus in an `.ogg` file is sent as it is; anything else needs FFmpeg to convert it)
- **react_to_message**: add a reaction to a message, or remove it

### Media

#### Sending

Files are only sent from the media folder (`media_dir`), and only with [full control](#full-control-sending) on.

- **Photos, videos and documents**: `send_file`. A document arrives with its file name and type.
- **Voice messages**: `send_audio_message`. WhatsApp plays a voice message only when it is Opus audio; an `.ogg` file with Opus is sent as it is. Anything else (MP3, WAV, Ogg Vorbis) is converted with [FFmpeg](https://ffmpeg.org) first, so FFmpeg must be installed for those. Without FFmpeg, send the file with `send_file`: it arrives as a file to play, not as a voice message.

#### Downloading

Received media is not downloaded right away: `messages.db` only holds its details. Messages with media show their type, message ID and chat JID, such as `[image - Message ID: 3EB0… - Chat JID: 31612345678@s.whatsapp.net]`. Ask Claude to download it: `download_media` takes the message ID and chat JID, and returns the path of the file.

- The file is saved in the store folder, in a folder for the chat. Two files with the same name in a chat each keep their own file.
- WhatsApp keeps media on its servers for a while only. For older media the bridge asks the sender's phone to upload it again, which works while that phone still has it.

## Troubleshooting

- Use an absolute path for `"command"` in the client configuration: MCP clients start the server from a folder of their own choosing.
- Make sure the bridge is running (`whatsapp-mcp service status`, or the terminal where you started `whatsapp-mcp bridge`): it stores what arrives, and sends for the MCP server.
- `serve` and `bridge` must use the same store folder. Both log it on start (`Store: …`). If your store is not in the default place, set `store_dir` in the configuration file.
- The MCP server writes its warnings and errors to stderr. Claude Desktop keeps them in its MCP log (`~/Library/Logs/Claude/mcp-server-whatsapp.log` on macOS); a missing `messages.db` is reported there when the server starts, and every read tool then says it cannot open the database.

### Linking

- **No QR code**: link in a terminal, with `whatsapp-mcp link`. The QR code is drawn with text blocks; if your terminal shows it garbled, make the window larger or use another terminal. Or link with a code instead: set `pair_phone` to your number with country code, and enter the 8-character code on your phone (Linked devices > Link a device > Link with phone number instead).
- **The QR code expired** (`Timeout waiting for QR code scan`): run `whatsapp-mcp link` again.
- **The pairing code is refused**: `pair_phone` needs the country code and no leading 0, such as `31612345678`, not `0612345678`.
- **Already linked**: `whatsapp-mcp link` says so and stops; start the bridge with `whatsapp-mcp service install` or `whatsapp-mcp bridge`.
- **`Not linked yet, and there is no terminal`**: the bridge runs as a service but was never linked (or was unlinked). Run `whatsapp-mcp link` in a terminal, then `whatsapp-mcp service install` again.
- **`Device logged out`**: the phone unlinked this bridge (Linked devices on the phone). Stop the bridge, run `whatsapp-mcp link`, and start it again.
- **Device limit reached**: WhatsApp allows a few linked devices. Remove one on your phone (Linked devices), then link again.
- **Few or no messages after linking**: WhatsApp sends your recent history in parts, which can take several minutes; `whatsapp-mcp link` waits until it is in. Only the recent part is sent: older messages are not in the store.
- **WhatsApp out of sync**: stop the bridge, delete `session.db` and `messages.db` in the store folder, and link again. This also deletes your stored history; WhatsApp sends the recent part again.

For additional Claude Desktop integration troubleshooting, see the [MCP documentation](https://modelcontextprotocol.io/quickstart/server#claude-for-desktop-integration-issues). The documentation includes helpful tips for checking logs and resolving common issues.
