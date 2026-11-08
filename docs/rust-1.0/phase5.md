# Phase 5: the Rust bridge

`whatsapp-mcp bridge` replaces the Go bridge in `whatsapp-bridge/`. It keeps the WhatsApp connection with [whatsapp-rust](https://github.com/oxidezap/whatsapp-rust), stores messages in the same `messages.db`, and serves the same local API to `whatsapp-mcp serve`. Part of the [plan](plan.md); the rows are in the [score sheet](scoresheet.md) (`W`, `A` and `S`).

**Done when** the Go bridge's tests are ported and pass, and the Rust bridge has run for a week alongside daily use without problems.

## Steps

Each step is one pull request and ends with something that works and is tested. The Go bridge stays in use until step 7.

| Step | Work | Done when |
|---|---|---|
| 1. Writing `messages.db` — **done** | The `store` crate also writes: the schema and migrations the Go bridge applies on start (tables, the `julianday` indexes, `quoted_message_id`, FTS5 with its triggers and rebuild), storing chats and messages with the same values (times as local time with offset, as Go writes them), media details. New tables `contacts` and `lid_map` for names, written by the bridge from step 3 on. `serve` reads names from them when they exist, and from whatsmeow's `whatsapp.db` otherwise. | The schema matches `parity/fixture/schema.sql` (plus the two new tables). Store tests ported from Go: FTS, `julianday` indexes, quoted IDs. A database written by Rust reads the same in `serve`; the parity harness still passes. |
| 2. Local API — **done** | The HTTP API on `127.0.0.1:8080`, exactly as [interface.md, section 3](interface.md#3-bridge-api) describes: token (constant-time), JSON only, 1 MiB, the status codes and texts, the send allowlist (with LID and phone number matching through `lid_map`), media paths only inside `media_dir`. WhatsApp sits behind a trait, so all of this is tested without a connection. | API tests ported from Go (`main_test.go`, `allowlist_test.go`, the API parts of the others) pass, plus new ones for every row of section 3. |
| 3. Connection and receiving — **done** | `whatsapp-mcp bridge`: pairing with a QR code or `pair_phone`, the session in its own file (`store/session.db`), reconnecting. Storing incoming messages: text, captions, non-text messages as bracketed text, quoted IDs, media details, edits and deletes; history sync; chat and group names; contacts, push names and the LID map into `contacts` and `lid_map`. | Message-conversion tests ported (`caption_test.go`, `reaction_test.go`, `quoted_test.go` and the parts of `main_test.go` on content). Live: phase 1 checklist items 1–12 against the test account. |
| 4. Sending and media — **done, except check 19** | Text with reply and mentions, files (image, video, document, voice note with Ogg duration and waveform), reactions; sent messages stored in the existing phone-number or LID chat; downloads into `<store>/<chat>/<file>`, with re-requesting expired media. | Tests ported (`outgoing_test.go`, `send_context_test.go`, `media_*_test.go`). Live: phase 1 checklist items 13–19, and the MCP tools end to end. |
| 5. Security and operation — **done** | Folder `0700` and files `0600`; logs without message content unless `log_content`, and without control characters; a token shorter than 32 characters stops the bridge; `serve` and `bridge` log the settings and paths they run with on start (see [Step 5: startup log](#step-5-startup-log)). | `fsperm_test.go` and `log_test.go` ported; a review of everything the bridge logs; the startup block tested for both commands, including that it never shows the token. |
| 6. Logging | Every log line has a time, a level and its source, the same for `serve`, `bridge` and the library. Where logs go (`stderr`, `stdout`, a file or syslog), their format (text or JSON) and the level are settings. See [Step 6: logging](#step-6-logging). | Tests for each output, format and level, the setting rules (shared with the Go bridge, which accepts and ignores the new settings), and that `serve` never logs to stdout; tried on macOS and Linux with syslog. |
| 7. Week of use | The Rust bridge replaces the Go bridge in daily use for a week; problems are fixed as they come. | A week without problems; the score sheet's `W`, `A` and `S` rows are `verified`. Then phase 6 (packaging) and 7 (release 1.0). |

## Step 3: live test

Passed: checks 1–12 with the test account, on 2026-11-06.

The bridge links, receives and stores; sending answers "not supported yet" until step 4. Run it with the **test account**, in a store folder of its own, while the Go bridge is stopped (both use port 8080):

```sh
cargo build --release
export WHATSAPP_BRIDGE_TOKEN=<32 or more characters>
./target/release/whatsapp-mcp bridge --store ~/wa-rust-test/store
# or link with a code: WHATSAPP_PAIR_PHONE=316... ./target/release/whatsapp-mcp bridge --store ...
```

Then check, as phase 1 items 1–12 did, but now in `messages.db` (`sqlite3 ~/wa-rust-test/store/messages.db`):

| # | Check | Expected |
|---|---|---|
| 1–2 | Link with the QR code, and once (after deleting the folder) with `WHATSAPP_PAIR_PHONE` | `Successfully connected and authenticated!`, then `✓ Connected to WhatsApp` |
| 3 | Ctrl+C, start again | No QR code; connected within seconds |
| 4 | Wi-Fi off for 2 minutes, a message sent meanwhile, Wi-Fi on | The message is stored after reconnecting |
| 5 | History sync after linking | `History sync complete. Stored N messages.`; `SELECT count(*) FROM messages` matches |
| 6 | Text, direct and in the group, also sent from the test phone itself | `← message stored` / `→ message stored`; rows with the right `chat_jid`, `sender`, `is_from_me` |
| 7 | Photo with a caption | `content` is the caption, `media_type` is `image` |
| 8 | Reaction, poll, vote, location, contact card, sticker, edit, delete | `content` is `[reaction: 👍]`, `[poll: …]`, `[poll vote]`, `[location: …]`, `[contact: …]`, `[sticker]`, `[edited] …`, `[message deleted]` |
| 9 | Reply on the phone | `quoted_message_id` is the ID of the message replied to |
| 10 | Messages from a LID sender | `SELECT * FROM lid_map` has the LID and the number |
| 11 | Names | `SELECT * FROM contacts` has push names (and saved names after the app state sync) |
| 12 | Groups | `SELECT jid, name FROM chats` shows the group's subject, not `Group 1203…` |

And the MCP server on the same store: `whatsapp-mcp serve --store ~/wa-rust-test/store`, with `list_chats` and `list_messages` showing names instead of numbers.

## Step 4: live test

Passed on 2026-11-07 with the test account: checks 13–18, the media folder refusal and the allowlist refusal (the MCP client showed the reason and what to change). Check 19 is still open: it needs media that has expired on WhatsApp's servers, which takes time to happen.

The bridge now sends, reacts and downloads. Test it through the MCP tools, with `whatsapp-mcp serve` talking to the Rust bridge, on the **test account** and its store from step 3 (the Go bridge stopped):

```sh
cargo build --release
export WHATSAPP_BRIDGE_TOKEN=<the same token for both>
export WHATSAPP_MEDIA_DIR=~/wa-rust-test/media   # the files to send, and nothing else
./target/release/whatsapp-mcp bridge --store ~/wa-rust-test/store
# in the MCP client: whatsapp-mcp serve --store ~/wa-rust-test/store, with the same token
```

Put a photo, a short video (MP4), a PDF and a voice note (`.ogg`, or any audio file for `send_audio_message` with ffmpeg installed) in the media folder. Then check, as phase 1 items 13–19 did:

| # | Check | Expected |
|---|---|---|
| 13 | `send_message` with text, to a number and to the group | Arrives on the phone; a row with `is_from_me` 1 in the chat that already exists for the contact (its LID chat when received messages are stored there) |
| 14 | `send_message` as a reply, direct and in the group | Shown as a reply on the phone; `quoted_message_id` stored |
| 15 | `send_message` with a mention in the group | The mention is highlighted on the phone |
| 16 | `send_file` with the photo, video and PDF (with a caption); `send_audio_message` with the voice note | Each arrives and opens; the PDF keeps its name and type; the voice note plays and shows its length |
| 17 | `react_to_message` with 👍, then with an empty emoji, direct and in the group | The reaction shows, then disappears; `[reaction: 👍]` stored |
| 18 | `download_media` for a photo, video, voice note and document sent from the phone | A path in `store/<chat>/`; the file opens and is readable by the owner only (`ls -l`); asking again gives the same path |
| 19 | `download_media` on an old photo from history sync, with the phone online | The log says `asking the sender's phone to upload it again...`, then the download succeeds |

Also: with `WHATSAPP_SEND_ALLOWLIST` set to one chat, sending to another is refused (403, the allowlist message); a file outside the media folder is refused (`Media path rejected`).

## Step 5: startup log

On start, `whatsapp-mcp serve` and `whatsapp-mcp bridge` log the settings and paths they run with. A wrong store folder, read-only mode switched on by accident, or an unset media folder then shows in the log. Today `serve` only warns when there is no `messages.db` in the store folder. `bridge` logs the configuration file, the allowlist size, `Starting WhatsApp client...` and the address of its API, but not the store path, the other settings, or whether it is linked.

One short block, for both commands:

| Line | Shows |
|---|---|
| Version | `whatsapp-mcp <version>` and the command (`serve` or `bridge`) |
| Configuration file | Its path, or that none was found. Environment variables take precedence over it. |
| `read_only` | On or off |
| `media_dir` | The resolved path, or "not set (sending files is disabled)" |
| `send_allowlist` | The number of chats, or "not set (all chats)" |
| `log_content` | On or off |
| `pair_phone` | `bridge` only: set or not set |
| Store | The full path of the store folder, and whether `messages.db` exists (for `serve`, this replaces today's warning) |
| Names | `serve` only: from the `contacts` and `lid_map` tables, or from the Go bridge's `whatsapp.db` |
| Bridge | `serve` only: the bridge address it sends to (`--bridge-url`, by default `http://127.0.0.1:8080/api`) |
| Session | `bridge` only: the path of `session.db`, and whether it is linked or will show a QR or pairing code |

Each setting also says where its value comes from: the environment, the configuration file, or the default.

Rules:

- **No secrets:** the bridge token is never logged, not even in part. The log only says whether it is set and long enough.
- **No message content:** paths and settings only.
- **Control characters removed:** values from the environment or the configuration file are cleaned like every other log line.
- **`serve` logs to stderr only:** stdout carries the MCP protocol.
- **Short:** a handful of lines, so the MCP client's log is not flooded.

Where: `crates/config` learns where each value came from; the `serve` block goes in `crates/mcp` or `crates/whatsapp-mcp`, the `bridge` block in `crates/bridge/src/run.rs`.

### Built

The block above, with the token shown only as `set`, `not set` or `too short`, and the pairing number only as set or not. The bridge logs its block before checking the settings, so a refused setting shows next to the others. Tests: `crates/config/tests/load.rs` (sources and the lines), `crates/whatsapp-mcp/src/main.rs` (`serve`), `crates/bridge/src/run.rs` (`bridge`).

### Log review

Every line the bridge logs was checked:

- **Content:** message text, captions, emoji, file names and paths, and the send and download status texts are logged only with `log_content`, as in the Go bridge. Without it, a message is `← message stored` (with `[image]` and so on for media).
- **Identifiers:** message IDs and chat JIDs are logged without `log_content`, as in the Go bridge. They are not content, but they are phone numbers: treat the log as private.
- **Secrets:** the token and the pairing number are never logged. Upload results (URL and media key) are never logged. The library's media errors carry only status codes and error kinds.
- **Control characters:** removed from every value and error text, and again from each whole line, keeping only the line breaks of the bridge's own text (the QR code).
- **The library's own log** (stderr, warnings and errors only by default): it names message IDs, JIDs and error kinds, not message content. `RUST_LOG=debug` logs much more and is for debugging, not for logs that are shared.

## Step 6: logging

Today only the library's warnings have a time. The bridge's own lines go to stdout without one, and `serve`'s go to stderr without one. After this step every line looks the same, like syslog:

```
2026-11-08T14:03:12.345+01:00 INFO  bridge: ✓ Connected to WhatsApp
2026-11-08T14:03:13.020+01:00 WARN  whatsapp-rust: Failed processing group plaintext (batch): …
```

That is the local time with its UTC offset and milliseconds, the level, the source (`serve`, `bridge` or `whatsapp-rust`) and the text. Control characters stay removed, and `log_content` keeps its meaning: the format changes, what is logged does not.

### Settings

Three new settings, with the usual rules: the configuration file or the environment, and the environment wins. All three are shown in the startup block.

| Setting | Environment variable | Values | Default |
|---|---|---|---|
| `log_output` | `WHATSAPP_LOG_OUTPUT` | `stderr`, `stdout`, `syslog`, or a file path | `stderr` |
| `log_format` | `WHATSAPP_LOG_FORMAT` | `text` (above) or `json`, one object per line with `time`, `level`, `source` and `message` | `text` |
| `log_level` | `WHATSAPP_LOG_LEVEL` | `error`, `warn`, `info` or `debug` | `info`; the library's own lines from `warn` |

- **A file** is appended to, and created readable by the owner only (`0600`), as the store is. `~/` means the home folder. Rotation is left to `logrotate` or `newsyslog`.
- **syslog** is the local one: `/dev/log` on Linux, `/var/run/syslog` on macOS. Lines are tagged `whatsapp-mcp` with the process ID and the facility `user`, and syslog adds its own time. Not available on Windows; asking for it there is an error at start.
- **An unknown value** in any of the three is an error at start that names the setting, as for the other settings.
- **`RUST_LOG`** keeps working for debugging and overrides `log_level` per module.

### Rules

- **`serve` never logs to stdout**, which carries the MCP protocol. `log_output: stdout` is refused when `serve` starts.
- **QR and pairing codes go to the terminal**, whatever `log_output` says, since they are needed to link the bridge. The log gets a line saying one is waiting.
- **The bridge moves from stdout to stderr** by default, like `serve`. Changelog: scripts that read the bridge's stdout must read stderr, or set `log_output: stdout`.
- **The Go bridge** reads the same configuration file and refuses settings it does not know. It learns the three names and ignores them, so one file keeps working for both until 1.0. The shared cases in `parity/config-cases.json` gain them.

### How

All logging goes through Rust's `log` facade (`log::info!` and so on), including what is now `log::line` and `eprintln!`. A small logger of our own does the format and the output and replaces `env_logger`, so the library's lines get the same format. The settings go in `crates/config`, the logger in a small shared module used by both commands.

Docs: interface.md section 2 (the settings), the README (Logs, Configuration), `config.example.yaml`, the score sheet (C13–C15, A08) and the changelog.

## Decided

- **Re-linking:** whatsapp-rust keeps its session in its own format, so the Go bridge's link (`whatsapp.db`) cannot be carried over. The Rust bridge is linked once, with a QR code or `pair_phone`. Decided on 2026-11-05.
- **Session file:** `store/session.db`, next to `messages.db`. `whatsapp.db` is the Go bridge's and is left alone, so going back to the Go bridge stays possible.
- **Event delivery:** the bridge takes every event from whatsapp-rust as it is dispatched and stores them in order, in one task. The library's callback delivery drops the newest event when its queue (256) is full, which a large history sync can reach.
- **Time of sent messages:** whatsapp-rust does not return the server's time for a sent message, so the bridge stores the local time of sending. The Go bridge stored whatsmeow's server time; the two differ by the time it takes to send.
- **Voice note length:** read from the Ogg file as RFC 7845 describes (pre-skip from the Opus header, granule positions at 48 kHz). The Go bridge read the pre-skip and rate a few bytes too far on.
- **Names in `messages.db`:** `serve` no longer depends on a library's private tables (see [plan, Contact names](plan.md#architecture)).

## Open

- **Daily use with a real account (step 7).** Phase 1 and steps 3–4 use the test account, because whatsapp-rust is an unofficial client and Meta may restrict accounts that use one. The whatsmeow-based Go bridge has the same risk, and this project already accepts it. Step 6 needs the account that is used every day; this needs a decision before step 6 starts.
