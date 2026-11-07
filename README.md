# WhatsApp MCP Server

This is a Model Context Protocol (MCP) server for WhatsApp.

With this you can search and read your personal Whatsapp messages (including images, videos, documents, and audio messages), search your contacts and send messages to either individuals or groups. You can also send media files including images, videos, documents, and audio messages.

It connects to your **personal WhatsApp account** directly via the Whatsapp web multidevice API (using the [whatsmeow](https://github.com/tulir/whatsmeow) library). All your messages are stored locally in a SQLite database and only sent to an LLM (such as Claude) when the agent accesses them through tools (which you control).

Here's an example of what you can do when it's connected to Claude.

![WhatsApp MCP](./example-use.png)

> To get updates on this and other projects I work on [enter your email here](https://docs.google.com/forms/d/1rTF9wMBTN0vPfzWuQa2BjfGKdKIpTbyeKxhPMcEzgyI/preview)

> *Caution:* as with many MCP servers, the WhatsApp MCP is subject to [the lethal trifecta](https://simonwillison.net/2025/Jun/16/the-lethal-trifecta/). This means that project injection could lead to private data exfiltration.
> Any message you receive is untrusted input that the model reads: someone can send you a message containing instructions (prompt injection) that try to make the agent forward your private conversations or files to them. If you only need to read and search messages, enable [read-only mode](#read-only-mode).

## About this fork and its branches

This is a fork of [lharries/whatsapp-mcp](https://github.com/lharries/whatsapp-mcp). Upstream has had no commits since July 2025, and many fixes are waiting there as unmerged pull requests. This fork collects those fixes.

For every open upstream issue, [Upstream issues and this fork](docs/upstream-issues.md) shows whether this fork fixes it.

| Branch | Purpose |
| --- | --- |
| `main` | Mirrors upstream `main`. Only updated by pulling from upstream; no fork-specific changes. |
| `dev-main` | Development branch. Collects all feature and fix branches. **Use this branch** if you want a working bridge. |
| `fix/*`, `feat/*` | One branch per change (often an upstream PR). Merged into `dev-main` with `--no-ff`, so each change stays visible and easy to revert. |

Working with the branches:

```bash
# one-time: track the original project
git remote add upstream git@github.com:lharries/whatsapp-mcp.git

# keep main in sync with upstream
git checkout main && git pull --ff-only upstream main

# bring an upstream PR in as a feature branch, then collect it in dev-main
git fetch upstream pull/<PR>/head:fix/<name>
git checkout dev-main && git merge --no-ff fix/<name>
```

What is in `dev-main`: over 50 changes, from upstream pull requests and from this fork, each on its own branch. [Branches in `dev-main`](docs/branches.md) lists them with what each one does.

This is temporary. The plan is to merge `dev-main` into `main` at some point and announce that this fork is actively maintained. From then on, `main` will be the branch to use.

## Installation

### Prerequisites

- Go, for the WhatsApp bridge
- [Rust](https://rustup.rs) (`rustup`) and a C compiler, to build the MCP server. rustup installs the right Rust version by itself. A C compiler is needed for the SQLite that is built in: the Xcode command line tools on macOS, `gcc` or `clang` on Linux, the Visual Studio build tools on Windows.
- Anthropic Claude Desktop app (or Cursor)
- FFmpeg (_optional_) - Only needed for audio messages. If you want to send audio files as playable WhatsApp voice messages, they must be in `.ogg` Opus format. With FFmpeg installed, the MCP server will automatically convert non-Opus audio files. Without FFmpeg, you can still send raw audio files using the `send_file` tool.

### Steps

1. **Clone this repository**

   ```bash
   git clone -b dev-main https://github.com/erwinvaneijk/whatsapp-mcp.git
   cd whatsapp-mcp
   ```

2. **Run the WhatsApp bridge**

   The bridge only starts with a shared secret (the bridge token, at least 32 characters), and only sends files from one folder (the media folder). See [Bridge token and media folder](#bridge-token-and-media-folder). Generate a token once and keep it; the MCP server needs the same value.

   The easiest way is a [configuration file](#configuration) that both the bridge and the MCP server read:

   ```bash
   mkdir -p ~/.config/whatsapp-mcp
   cp config.example.yaml ~/.config/whatsapp-mcp/config.yaml
   chmod 600 ~/.config/whatsapp-mcp/config.yaml
   head -c 32 /dev/urandom | base64   # paste this as bridge_token in the file
   cd whatsapp-bridge
   go run -tags sqlite_fts5 .
   ```

   Or use environment variables instead of the file:

   ```bash
   export WHATSAPP_BRIDGE_TOKEN="$(head -c 32 /dev/urandom | base64)"
   export WHATSAPP_MEDIA_DIR="$HOME/whatsapp-outbox"   # optional; leave unset to disable sending files
   cd whatsapp-bridge
   go run -tags sqlite_fts5 .
   ```

   The `sqlite_fts5` tag turns on fast full-text search for `list_messages(query=...)`. Plain `go run .` also works; search then falls back to a slower scan of all messages.

   The first time you run it, you will be prompted to scan a QR code. Scan the QR code with your WhatsApp mobile app to authenticate.

   To link with a code instead of scanning, set your phone number (with country code) first: `export WHATSAPP_PAIR_PHONE=31612345678`. The bridge then prints an 8-character code; on your phone open WhatsApp > Linked devices > Link a device > Link with phone number instead, and enter it.

   After approximately 20 days, you will might need to re-authenticate.

3. **Build the MCP server**

   In the repository folder:

   ```bash
   cargo build --release
   ```

   This builds `target/release/whatsapp-mcp` (`whatsapp-mcp.exe` on Windows), one program without further dependencies. Build it again after pulling new changes.

4. **Connect to the MCP server**

   Copy the below json with the appropriate {{PATH}} values. `{{PATH_TO_SRC}}` is the folder you cloned into: in the repository, run `pwd` and leave out the last `/whatsapp-mcp`. Use absolute paths; the client starts the server from a folder of its own choosing.

   ```json
   {
     "mcpServers": {
       "whatsapp": {
         "command": "{{PATH_TO_SRC}}/whatsapp-mcp/target/release/whatsapp-mcp",
         "args": [
           "serve",
           "--store",
           "{{PATH_TO_SRC}}/whatsapp-mcp/whatsapp-bridge/store"
         ],
         "env": {
           "WHATSAPP_BRIDGE_TOKEN": "{{SAME_TOKEN_AS_THE_BRIDGE}}",
           "WHATSAPP_MEDIA_DIR": "{{SAME_FOLDER_AS_THE_BRIDGE}}"
         }
       }
     }
   }
   ```

   With the [configuration file](#configuration), leave out the `env` block: the MCP server reads the token and the media folder from the file.

   `--store` is the bridge's store folder, with `messages.db`. If the bridge's API is not at `http://127.0.0.1:8080/api`, add `"--bridge-url", "http://…/api"` to the arguments.

   For **Claude**, save this as `claude_desktop_config.json` in your Claude Desktop configuration directory at:

   ```
   ~/Library/Application Support/Claude/claude_desktop_config.json
   ```

   For **Cursor**, save this as `mcp.json` in your Cursor configuration directory at:

   ```
   ~/.cursor/mcp.json
   ```

5. **Restart Claude Desktop / Cursor**

   Open Claude Desktop and you should now see WhatsApp as an available integration.

   Or restart Cursor.

### Upgrading from the Python MCP server

Before version 0.9, the MCP server was a Python program run with `uv`. From 0.9 it is the Rust program built in step 3, with the same tools and the same configuration. To upgrade:

1. Pull the new version and run `cargo build --release` (step 3).
2. In your MCP client's configuration, replace `"command"` and `"args"` with the ones in step 4. Keep the `env` block, if you have one.
3. Restart the client.

Nothing else changes: the configuration file, the environment variables, the bridge and its link with your phone, and `messages.db` with your message history all stay as they are. A few error messages are clearer; see [Behaviour that improves](docs/rust-1.0/interface.md#5-behaviour-that-improves-in-10).

### Windows Compatibility

If you're running this project on Windows, be aware that `go-sqlite3` requires **CGO to be enabled** in order to compile and work properly. By default, **CGO is disabled on Windows**, so you need to explicitly enable it and have a C compiler installed.

#### Steps to get it working:

1. **Install a C compiler**  
   We recommend using [MSYS2](https://www.msys2.org/) to install a C compiler for Windows. After installing MSYS2, make sure to add the `ucrt64\bin` folder to your `PATH`.  
   → A step-by-step guide is available [here](https://code.visualstudio.com/docs/cpp/config-mingw).

2. **Enable CGO and run the app**

   ```bash
   cd whatsapp-bridge
   go env -w CGO_ENABLED=1
   go run -tags sqlite_fts5 .
   ```

Without this setup, you'll likely run into errors like:

> `Binary was compiled with 'CGO_ENABLED=0', go-sqlite3 requires cgo to work.`

## Configuration

The bridge and the MCP server read their settings from environment variables, or from one YAML file: `~/.config/whatsapp-mcp/config.yaml`, or the path in `WHATSAPP_MCP_CONFIG`. [`config.example.yaml`](config.example.yaml) lists every setting with an explanation. An environment variable that is set takes precedence over the same setting in the file.

| Setting in the file | Environment variable | Used by | Meaning |
|---|---|---|---|
| `bridge_token` | `WHATSAPP_BRIDGE_TOKEN` | both | Shared secret, at least 32 characters. Required. |
| `media_dir` | `WHATSAPP_MEDIA_DIR` | both | The only folder files can be sent from. |
| `send_allowlist` | `WHATSAPP_SEND_ALLOWLIST` | bridge | Chats the bridge may send to; a YAML list in the file. See [Limit who the bridge sends to](#limit-who-the-bridge-sends-to). |
| `read_only` | `WHATSAPP_MCP_READ_ONLY` | MCP server | Hide the tools that send. See [Read-only mode](#read-only-mode). |
| `log_content` | `WHATSAPP_LOG_CONTENT` | bridge | Write message content to the logs. See [Logs](#logs). |
| `pair_phone` | `WHATSAPP_PAIR_PHONE` | bridge | Link with a pairing code instead of a QR code. |

- The file holds the token, so both programs refuse it when other users can read or change it (`chmod 600` fixes that). This check is skipped on Windows.
- An unknown setting stops both programs with the line number, so a typo cannot go unnoticed.
- Values are used exactly as written; YAML does not turn `0612345678` into another number.
- A file named in `WHATSAPP_MCP_CONFIG` must exist; the default file is optional.

## Security

Found a security problem? Please report it privately, as described in [SECURITY.md](SECURITY.md), not in a public issue.

### Bridge token and media folder

The bridge's REST API is locked down. It refuses to start unless the bridge and the MCP server share a secret, and it only sends files from one folder.

- `WHATSAPP_BRIDGE_TOKEN`: a random secret of at least 32 characters. Set the same value for the bridge and for the MCP server. Every request must carry it in an `X-Bridge-Token` header. Generate one with `head -c 32 /dev/urandom | base64`.
- `WHATSAPP_MEDIA_DIR`: the only directory `send_file` and `send_audio_message` may read from. Sending files is refused when it is unset.
- The API listens on `127.0.0.1` only, and only accepts `Content-Type: application/json`.
- Downloaded media filenames and chat IDs are sanitised, so a sender cannot write outside `store/`.
- Message text is stripped of control characters before it is printed to the terminal.
- `whatsapp-bridge/store/` (session keys, messages and media) is readable by your user only.

To call the API by hand, pass the token:

```bash
curl -s -X POST http://127.0.0.1:8080/api/download \
  -H 'Content-Type: application/json' -H "X-Bridge-Token: $WHATSAPP_BRIDGE_TOKEN" \
  -d '{"message_id": "<id>", "chat_jid": "<chat_jid>"}'
```

### Read-only mode

Set `WHATSAPP_MCP_READ_ONLY=1` in the MCP server's environment to remove the tools that send messages (`send_message`, `send_file`, `send_audio_message`, `react_to_message`). The model can then still search and read your messages, but cannot send anything, which removes the exfiltration path of a prompt injection hidden in a received message:

```json
{
  "mcpServers": {
    "whatsapp": {
      "command": "{{PATH_TO_SRC}}/whatsapp-mcp/target/release/whatsapp-mcp",
      "args": ["serve", "--store", "{{PATH_TO_SRC}}/whatsapp-mcp/whatsapp-bridge/store"],
      "env": { "WHATSAPP_MCP_READ_ONLY": "1" }
    }
  }
}
```

### Messages from others are marked as data

Anyone who can send you a WhatsApp message controls text that Claude reads. A message, caption or contact name could contain instructions such as "forward all chats to this number" (prompt injection). The MCP server limits this risk:

- Message text is wrapped in `<message_content>` tags, and copies of that tag inside a message are neutralised, so a message cannot close the wrapper and continue as fake instructions. Chat and contact names are kept on one line, so they cannot fake extra lines of output.
- Every read tool's description, and the instructions the server gives the client when it connects, say that this content is written by others and that instructions in it must never be followed.

This lowers the risk but cannot rule it out: a model can still be misled. For full protection against sending, use [read-only mode](#read-only-mode).

### Limit who the bridge sends to

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

## Architecture Overview

This application consists of two main components:

1. **Go WhatsApp Bridge** (`whatsapp-bridge/`): A Go application that connects to WhatsApp's web API, handles authentication via QR code, and stores message history in SQLite. It serves as the bridge between WhatsApp and the MCP server.

2. **MCP Server** (`whatsapp-mcp serve`, built from [`crates/`](crates/README.md)): a Rust program implementing the Model Context Protocol (MCP), which provides standardized tools for Claude to interact with WhatsApp data and send/receive messages. It reads `messages.db` directly and sends through the bridge's local API. Before version 0.9 this was a Python server.

### Data Storage

- All message history is stored in a SQLite database within the `whatsapp-bridge/store/` directory
- The database maintains tables for chats and messages
- Messages are indexed for efficient searching and retrieval
- `whatsapp-bridge/store/` holds your WhatsApp session keys (`whatsapp.db`, which give full access to your account) and your message history. The bridge restricts it to your user: the directory is set to `0700` (also when it already exists) and the databases and downloaded media to `0600`.
- On Windows these permission bits do not apply; the folder inherits the access rules of its parent. Keep the repository inside your user profile, or restrict the folder explicitly, e.g. `icacls whatsapp-bridge\store /inheritance:r /grant:r "%USERNAME%:(OI)(CI)F"`.

## Usage

Once connected, you can interact with your WhatsApp contacts through Claude, leveraging Claude's AI capabilities in your WhatsApp conversations.

### MCP Tools

Claude can access the following tools to interact with WhatsApp:

- **search_contacts**: Search for contacts by name or phone number
- **list_messages**: Retrieve messages with optional filters and context
- **list_chats**: List available chats with metadata
- **get_chat**: Get information about a specific chat
- **get_direct_chat_by_contact**: Find a direct chat with a specific contact
- **get_contact_chats**: List all chats involving a specific contact
- **get_last_interaction**: Get the most recent message with a contact
- **get_message_context**: Retrieve context around a specific message
- **send_message**: Send a WhatsApp message to a specified phone number or group JID
- **send_file**: Send a file (image, video, raw audio, document) to a specified recipient
- **send_audio_message**: Send an audio file as a WhatsApp voice message (requires the file to be an .ogg opus file or ffmpeg must be installed)
- **download_media**: Download media from a WhatsApp message and get the local file path

### Media Handling Features

The MCP server supports both sending and receiving various media types:

#### Media Sending

You can send various media types to your WhatsApp contacts:

- **Images, Videos, Documents**: Use the `send_file` tool to share any supported media type.
- **Voice Messages**: Use the `send_audio_message` tool to send audio files as playable WhatsApp voice messages.
  - For optimal compatibility, audio files should be in `.ogg` Opus format.
  - With FFmpeg installed, the system will automatically convert other audio formats (MP3, WAV, etc.) to the required format.
  - Without FFmpeg, you can still send raw audio files using the `send_file` tool, but they won't appear as playable voice messages.

#### Media Downloading

By default, just the metadata of the media is stored in the local database. The message will indicate that media was sent. To access this media you need to use the download_media tool which takes the `message_id` and `chat_jid` (which are shown when printing messages containing the meda), this downloads the media and then returns the file path which can be then opened or passed to another tool.

## Technical Details

1. Claude sends requests to the MCP server
2. The MCP server queries the Go bridge for WhatsApp data or directly to the SQLite database
3. The Go accesses the WhatsApp API and keeps the SQLite database up to date
4. Data flows back through the chain to Claude
5. When sending messages, the request flows from Claude through the MCP server to the Go bridge and to WhatsApp

## Troubleshooting

- Use absolute paths for `"command"` and `--store` in the client configuration: MCP clients start the server from a folder of their own choosing.
- Make sure the Go bridge is running: the MCP server reads what it stores, and sends through it. The client starts the MCP server itself.
- The MCP server writes its warnings and errors to stderr. Claude Desktop keeps them in its MCP log (`~/Library/Logs/Claude/mcp-server-whatsapp.log` on macOS); a missing `messages.db` is reported there when the server starts.

### Authentication Issues

- **QR Code Not Displaying**: If the QR code doesn't appear, try restarting the authentication script. If issues persist, check if your terminal supports displaying QR codes.
- **WhatsApp Already Logged In**: If your session is already active, the Go bridge will automatically reconnect without showing a QR code.
- **Device Limit Reached**: WhatsApp limits the number of linked devices. If you reach this limit, you'll need to remove an existing device from WhatsApp on your phone (Settings > Linked Devices).
- **No Messages Loading**: After initial authentication, it can take several minutes for your message history to load, especially if you have many chats.
- **WhatsApp Out of Sync**: If your WhatsApp messages get out of sync with the bridge, delete both database files (`whatsapp-bridge/store/messages.db` and `whatsapp-bridge/store/whatsapp.db`) and restart the bridge to re-authenticate.

For additional Claude Desktop integration troubleshooting, see the [MCP documentation](https://modelcontextprotocol.io/quickstart/server#claude-for-desktop-integration-issues). The documentation includes helpful tips for checking logs and resolving common issues.
