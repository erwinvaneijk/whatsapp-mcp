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

What is in `dev-main` now:

- `fix/405-whatsmeow-bump` — upstream PR [#381](https://github.com/lharries/whatsapp-mcp/pull/381): updates whatsmeow to fix the `Client outdated (405)` connect error. Requires Go 1.26 or newer.
- `fix/media-403` — upstream PR [#361](https://github.com/lharries/whatsapp-mcp/pull/361): fixes the 403 error on `/api/download` for media.
- `fix/media-same-name` — upstream PR [#370](https://github.com/lharries/whatsapp-mcp/pull/370): media with the same filename in a chat (such as photos sent together) each get their own file instead of overwriting each other.
- `fix/whatsmeow-1006` — upstream PR [#392](https://github.com/lharries/whatsapp-mcp/pull/392): a newer whatsmeow, for `websocket 1006` disconnects while connecting.
- `fix/media-cdn-auth` — upstream PR [#367](https://github.com/lharries/whatsapp-mcp/pull/367): tests that media downloads keep the CDN signature. The fix itself was already in `fix/media-403`.

Security:

- `fix/api-hardening` — upstream PR [#387](https://github.com/lharries/whatsapp-mcp/pull/387): the bridge API listens on `127.0.0.1` only, requires a shared token, only sends files from one folder, and keeps downloads inside `store/`. **Changes setup:** see [Bridge token and media folder](#bridge-token-and-media-folder).
- `fix/store-permissions` — upstream PR [#389](https://github.com/lharries/whatsapp-mcp/pull/389): `store/` and its databases and media are readable by your user only.
- `feat/read-only-mode` — upstream PR [#388](https://github.com/lharries/whatsapp-mcp/pull/388): optional [read-only mode](#read-only-mode) that hides the send tools.

Fixes and speed:

- `fix/logs-to-stderr` — upstream PR [#372](https://github.com/lharries/whatsapp-mcp/pull/372): the MCP server logs to stderr, so log lines no longer corrupt the MCP connection.
- `fix/last-message-join` — upstream PR [#283](https://github.com/lharries/whatsapp-mcp/pull/283): chat lists no longer duplicate or drop chats.
- `fix/list-chats-no-last-message` — upstream PR [#380](https://github.com/lharries/whatsapp-mcp/pull/380): `list_chats` and `get_chat` work with `include_last_message=False`.
- `fix/contact-chats-limits-timeouts` — upstream PR [#373](https://github.com/lharries/whatsapp-mcp/pull/373): no duplicate rows in `get_contact_chats`, negative limits are rejected, and calls to the bridge time out.
- `fix/document-mime` — upstream PR [#390](https://github.com/lharries/whatsapp-mcp/pull/390): documents are sent with their real file type and name instead of as unnamed `.bin` files.
- `fix/document-mime-type` — upstream PR [#328](https://github.com/lharries/whatsapp-mcp/pull/328): the same fix as #390; merged for its history only.
- `fix/voice-message-cleanup` — upstream PR [#369](https://github.com/lharries/whatsapp-mcp/pull/369): `send_audio_message` deletes the converted `.ogg` file after sending.
- `feat/fts5-search` — upstream PR [#281](https://github.com/lharries/whatsapp-mcp/pull/281): fast full-text search in `list_messages`. Needs the bridge built with `-tags sqlite_fts5`; without it, search uses the old slow scan. A search matches whole words or the start of a word, not text in the middle of a word.
- `feat/mcp-2x` — upstream PR [#378](https://github.com/lharries/whatsapp-mcp/pull/378): the MCP server uses mcp 2.x, so MCP clients on the 2.x protocol connect at once instead of waiting for a timeout. Needs Python 3.11 or newer.
- `fix/mcp2-tool-output-types` — fork fix: with mcp 2.x every read tool failed ("Error executing tool"), because the tools declared a different return type than they returned. Fixed, with a test that calls every tool through the MCP server.
- `ci/github-actions` — upstream PR [#150](https://github.com/lharries/whatsapp-mcp/pull/150): GitHub Actions builds and tests the bridge (with and without full-text search) and runs the Python tests on every push.
- `fix/gitignore-binary` — upstream PR [#349](https://github.com/lharries/whatsapp-mcp/pull/349): git ignores the compiled bridge program.
- `fix/sqlite-indexes` — upstream PR [#279](https://github.com/lharries/whatsapp-mcp/pull/279): database indexes for faster message and chat queries.
- `fix/sqlite-wal` — upstream PR [#393](https://github.com/lharries/whatsapp-mcp/pull/393): faster history sync and WAL mode for the databases.

Contacts and messages:

- `fix/contact-names-lid` — upstream PR [#375](https://github.com/lharries/whatsapp-mcp/pull/375): chats and senders show the saved contact name instead of a bare number, also for chats on WhatsApp's newer LID IDs. Searching by phone number finds LID chats too. Names come from the bridge's `whatsapp.db`; a newly saved contact can take up to 10 seconds to show up.
- `fix/media-captions` — upstream PR [#350](https://github.com/lharries/whatsapp-mcp/pull/350): captions on images, videos and documents are stored as the message text instead of being dropped, also for messages from history sync. Applies to newly received or re-synced messages.
- `feat/store-non-text-messages` — upstream PR [#345](https://github.com/lharries/whatsapp-mcp/pull/345): reactions, polls, poll votes, locations, contact cards, stickers, edits and deletes are stored as short bracketed text (such as `[reaction: ❤️]` or `[edited] new text`) instead of being dropped. Applies to newly received messages.
- `feat/reply-to-message` — upstream PR [#277](https://github.com/lharries/whatsapp-mcp/pull/277): `send_message` can reply to a specific message with `reply_to_message_id`. For group chats, also pass `reply_to_sender_jid` (the original sender).
- `feat/send-file-caption` — upstream PR [#200](https://github.com/lharries/whatsapp-mcp/pull/200): `send_file` takes an optional `caption`, shown with the image, video or document.
- `feat/send-mentions` — upstream PR [#321](https://github.com/lharries/whatsapp-mcp/pull/321): `send_message` takes `mentions`, a list of phone numbers or JIDs to @mention. Also put `@<number>` in the text, or WhatsApp does not highlight it. Works together with replies.
- `feat/send-reactions` — upstream PR [#148](https://github.com/lharries/whatsapp-mcp/pull/148): new `react_to_message` tool to react with an emoji, or remove your reaction with an empty one. The message must be in the bridge's database. Hidden in read-only mode.
- `feat/quoted-message-id` — upstream PR [#325](https://github.com/lharries/whatsapp-mcp/pull/325): replies are stored with the ID of the message they quote and shown as `(reply to <id>)`. Every message now shows its ID, to use with replies and reactions. Applies to newly received or re-synced messages.
- `feat/store-sent-messages` — upstream PR [#383](https://github.com/lharries/whatsapp-mcp/pull/383): messages and reactions sent through the bridge are stored, so they show up in `list_messages` and can be replied or reacted to. They go into the existing chat, whether that is stored under the phone number or the LID.
- `feat/tool-annotations` — upstream PR [#142](https://github.com/lharries/whatsapp-mcp/pull/142): every tool tells MCP clients whether it only reads, or sends messages that cannot be undone (so the client can ask first).
- `feat/media-retry-pair-code` — upstream PR [#273](https://github.com/lharries/whatsapp-mcp/pull/273): media that has expired on WhatsApp's servers is requested again from the sender's phone (which must be online), and the bridge can link with a code instead of a QR code (`WHATSAPP_PAIR_PHONE`).

This is temporary. The plan is to merge `dev-main` into `main` at some point and announce that this fork is actively maintained. From then on, `main` will be the branch to use.

## Installation

### Prerequisites

- Go
- Python 3.11+ (uv installs it for you if needed)
- Anthropic Claude Desktop app (or Cursor)
- UV (Python package manager), install with `curl -LsSf https://astral.sh/uv/install.sh | sh`
- FFmpeg (_optional_) - Only needed for audio messages. If you want to send audio files as playable WhatsApp voice messages, they must be in `.ogg` Opus format. With FFmpeg installed, the MCP server will automatically convert non-Opus audio files. Without FFmpeg, you can still send raw audio files using the `send_file` tool.

### Steps

1. **Clone this repository**

   ```bash
   git clone -b dev-main https://github.com/erwinvaneijk/whatsapp-mcp.git
   cd whatsapp-mcp
   ```

2. **Run the WhatsApp bridge**

   The bridge only starts with a shared secret (`WHATSAPP_BRIDGE_TOKEN`, at least 32 characters), and only sends files from one folder (`WHATSAPP_MEDIA_DIR`). See [Bridge token and media folder](#bridge-token-and-media-folder). Generate a token once and keep it; the MCP server needs the same value.

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

3. **Connect to the MCP server**

   Copy the below json with the appropriate {{PATH}} values:

   ```json
   {
     "mcpServers": {
       "whatsapp": {
         "command": "{{PATH_TO_UV}}", // Run `which uv` and place the output here
         "args": [
           "--directory",
           "{{PATH_TO_SRC}}/whatsapp-mcp/whatsapp-mcp-server", // cd into the repo, run `pwd` and enter the output here + "/whatsapp-mcp-server"
           "run",
           "main.py"
         ],
         "env": {
           "WHATSAPP_BRIDGE_TOKEN": "{{SAME_TOKEN_AS_THE_BRIDGE}}",
           "WHATSAPP_MEDIA_DIR": "{{SAME_FOLDER_AS_THE_BRIDGE}}"
         }
       }
     }
   }
   ```

   For **Claude**, save this as `claude_desktop_config.json` in your Claude Desktop configuration directory at:

   ```
   ~/Library/Application Support/Claude/claude_desktop_config.json
   ```

   For **Cursor**, save this as `mcp.json` in your Cursor configuration directory at:

   ```
   ~/.cursor/mcp.json
   ```

4. **Restart Claude Desktop / Cursor**

   Open Claude Desktop and you should now see WhatsApp as an available integration.

   Or restart Cursor.

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

## Security

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

Set `WHATSAPP_MCP_READ_ONLY=1` in the MCP server's environment to remove the tools that send messages (`send_message`, `send_file`, `send_audio_message`). The model can then still search and read your messages, but cannot send anything, which removes the exfiltration path of a prompt injection hidden in a received message:

```json
{
  "mcpServers": {
    "whatsapp": {
      "command": "{{PATH_TO_UV}}",
      "args": ["--directory", "{{PATH_TO_SRC}}/whatsapp-mcp/whatsapp-mcp-server", "run", "main.py"],
      "env": { "WHATSAPP_MCP_READ_ONLY": "1" }
    }
  }
}
```

## Architecture Overview

This application consists of two main components:

1. **Go WhatsApp Bridge** (`whatsapp-bridge/`): A Go application that connects to WhatsApp's web API, handles authentication via QR code, and stores message history in SQLite. It serves as the bridge between WhatsApp and the MCP server.

2. **Python MCP Server** (`whatsapp-mcp-server/`): A Python server implementing the Model Context Protocol (MCP), which provides standardized tools for Claude to interact with WhatsApp data and send/receive messages.

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

1. Claude sends requests to the Python MCP server
2. The MCP server queries the Go bridge for WhatsApp data or directly to the SQLite database
3. The Go accesses the WhatsApp API and keeps the SQLite database up to date
4. Data flows back through the chain to Claude
5. When sending messages, the request flows from Claude through the MCP server to the Go bridge and to WhatsApp

## Troubleshooting

- If you encounter permission issues when running uv, you may need to add it to your PATH or use the full path to the executable.
- Make sure both the Go application and the Python server are running for the integration to work properly.

### Authentication Issues

- **QR Code Not Displaying**: If the QR code doesn't appear, try restarting the authentication script. If issues persist, check if your terminal supports displaying QR codes.
- **WhatsApp Already Logged In**: If your session is already active, the Go bridge will automatically reconnect without showing a QR code.
- **Device Limit Reached**: WhatsApp limits the number of linked devices. If you reach this limit, you'll need to remove an existing device from WhatsApp on your phone (Settings > Linked Devices).
- **No Messages Loading**: After initial authentication, it can take several minutes for your message history to load, especially if you have many chats.
- **WhatsApp Out of Sync**: If your WhatsApp messages get out of sync with the bridge, delete both database files (`whatsapp-bridge/store/messages.db` and `whatsapp-bridge/store/whatsapp.db`) and restart the bridge to re-authenticate.

For additional Claude Desktop integration troubleshooting, see the [MCP documentation](https://modelcontextprotocol.io/quickstart/server#claude-for-desktop-integration-issues). The documentation includes helpful tips for checking logs and resolving common issues.
