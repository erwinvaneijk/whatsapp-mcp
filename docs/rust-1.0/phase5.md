# Phase 5: the Rust bridge

`whatsapp-mcp bridge` replaces the Go bridge in `whatsapp-bridge/`. It keeps the WhatsApp connection with [whatsapp-rust](https://github.com/oxidezap/whatsapp-rust), stores messages in the same `messages.db`, and serves the same local API to `whatsapp-mcp serve`. Part of the [plan](plan.md); the rows are in the [score sheet](scoresheet.md) (`W`, `A` and `S`).

**Done when** the Go bridge's tests are ported and pass, and the Rust bridge has run for a week alongside daily use without problems.

## Steps

Each step is one pull request and ends with something that works and is tested. The Go bridge stays in use until step 6.

| Step | Work | Done when |
|---|---|---|
| 1. Writing `messages.db` — **done** | The `store` crate also writes: the schema and migrations the Go bridge applies on start (tables, the `julianday` indexes, `quoted_message_id`, FTS5 with its triggers and rebuild), storing chats and messages with the same values (times as local time with offset, as Go writes them), media details. New tables `contacts` and `lid_map` for names, written by the bridge from step 3 on. `serve` reads names from them when they exist, and from whatsmeow's `whatsapp.db` otherwise. | The schema matches `parity/fixture/schema.sql` (plus the two new tables). Store tests ported from Go: FTS, `julianday` indexes, quoted IDs. A database written by Rust reads the same in `serve`; the parity harness still passes. |
| 2. Local API — **done** | The HTTP API on `127.0.0.1:8080`, exactly as [interface.md, section 3](interface.md#3-bridge-api) describes: token (constant-time), JSON only, 1 MiB, the status codes and texts, the send allowlist (with LID and phone number matching through `lid_map`), media paths only inside `media_dir`. WhatsApp sits behind a trait, so all of this is tested without a connection. | API tests ported from Go (`main_test.go`, `allowlist_test.go`, the API parts of the others) pass, plus new ones for every row of section 3. |
| 3. Connection and receiving — **built; live test open** | `whatsapp-mcp bridge`: pairing with a QR code or `pair_phone`, the session in its own file (`store/session.db`), reconnecting. Storing incoming messages: text, captions, non-text messages as bracketed text, quoted IDs, media details, edits and deletes; history sync; chat and group names; contacts, push names and the LID map into `contacts` and `lid_map`. | Message-conversion tests ported (`caption_test.go`, `reaction_test.go`, `quoted_test.go` and the parts of `main_test.go` on content). Live: phase 1 checklist items 1–12 against the test account. |
| 4. Sending and media | Text with reply and mentions, files (image, video, document, voice note with Ogg duration and waveform), reactions; sent messages stored in the existing phone-number or LID chat; downloads into `<store>/<chat>/<file>`, with re-requesting expired media. | Tests ported (`outgoing_test.go`, `send_context_test.go`, `media_*_test.go`). Live: phase 1 checklist items 13–19, and the MCP tools end to end. |
| 5. Security and operation | Folder `0700` and files `0600`; logs without message content unless `log_content`, and without control characters; a token shorter than 32 characters stops the bridge; clear messages on start (store, link status, allowlist size). | `fsperm_test.go` and `log_test.go` ported; a review of everything the bridge logs. |
| 6. Week of use | The Rust bridge replaces the Go bridge in daily use for a week; problems are fixed as they come. | A week without problems; the score sheet's `W`, `A` and `S` rows are `verified`. Then phase 6 (packaging) and 7 (release 1.0). |

## Step 3: live test

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

## Decided

- **Re-linking:** whatsapp-rust keeps its session in its own format, so the Go bridge's link (`whatsapp.db`) cannot be carried over. The Rust bridge is linked once, with a QR code or `pair_phone`. Decided on 2026-11-05.
- **Session file:** `store/session.db`, next to `messages.db`. `whatsapp.db` is the Go bridge's and is left alone, so going back to the Go bridge stays possible.
- **Event delivery:** the bridge takes every event from whatsapp-rust as it is dispatched and stores them in order, in one task. The library's callback delivery drops the newest event when its queue (256) is full, which a large history sync can reach.
- **Names in `messages.db`:** `serve` no longer depends on a library's private tables (see [plan, Contact names](plan.md#architecture)).

## Open

- **Daily use with a real account (step 6).** Phase 1 and steps 3–4 use the test account, because whatsapp-rust is an unofficial client and Meta may restrict accounts that use one. The whatsmeow-based Go bridge has the same risk, and this project already accepts it. Step 6 needs the account that is used every day; this needs a decision before step 6 starts.
