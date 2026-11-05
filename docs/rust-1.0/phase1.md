# Phase 1: whatsapp-rust go/no-go test

Can [whatsapp-rust](https://github.com/oxidezap/whatsapp-rust) do everything the bridge does today? This page is the checklist and the record of the results. Part of the [plan](plan.md); the rows map to the `W` rows of the [score sheet](scoresheet.md).

**Go** when every item works on macOS, Linux and Windows, or has a workaround. **No-go** otherwise: 1.0 then keeps the Go bridge and only replaces the Python server.

## Setup

- **Test account only.** Use the spare number. Never link your own account: whatsapp-rust is an unofficial client, and Meta may suspend accounts that use one.
- **Second phone or account.** Many tests need someone to talk to, such as your own number on your own phone, sending to and receiving from the test account. A group with both numbers covers the group tests.
- **Library version:** whatsapp-rust at commit `52fb6643` (2026-11-05), pinned in `spike/whatsapp-rust/Cargo.toml`.
- **Rust:** the test program needs Rust 1.95 or newer. `spike/whatsapp-rust/rust-toolchain.toml` selects 1.99 for that folder only.

## Running the test program

```bash
cd spike/whatsapp-rust
cargo run --release                         # link with a QR code
cargo run --release -- --pair-phone 316...  # or link with a pairing code (test number, with country code)
cargo run --release -- --show-content       # also print message text and names (test account only)
```

- **Storage:** the session lives in `spike/whatsapp-rust/store/`, which git ignores. Delete that folder to start over.
- **Output:** the program prints one line per event: messages, history sync, contacts. By default it shows message IDs, chat and sender IDs and message kinds, but not the text.
- **Commands:** type `help` to list them.

| Command | Does |
|---|---|
| `send <to> <text>` | Text message; `to` is a phone number or a JID |
| `reply <id> <text>` | Reply quoting a message seen earlier |
| `mention <to> <number> <text>` | Text that @mentions a number; put `@<number>` in the text too |
| `image`, `video`, `doc <to> <file> [caption]` | Media with an optional caption |
| `voice <to> <file.ogg>` | Voice note (Opus in Ogg) |
| `react <id> <emoji>`, `unreact <id>` | Add or remove a reaction |
| `download <id>` | Download a message's media into `store/downloads/` |
| `retry <id>` | Ask the sender's phone to upload expired media again, then download it |
| `group <jid>`, `groups` | Group name and member count; the groups this account is in |
| `lid <jid-or-number>` | Look up the LID-to-phone mapping |
| `recent [n]` | The last messages seen, with their IDs |
| `stats` | Counters since start: live messages, history sync, contacts |

## Checklist

Fill in one result per platform:
- ✅ works
- ⚠️ works with a workaround (say which in the Notes column)
- ❌ fails (say how in the Notes column)

| # | Score sheet | Test | How | macOS | Linux | Windows | Notes |
|---|---|---|---|---|---|---|---|
| 1 | W02 | Link with a QR code | Start without `--pair-phone`, scan the QR code | | | | |
| 2 | W03 | Link with a pairing code | Delete `store/`, start with `--pair-phone`, enter the code on the phone | | | | |
| 3 | W04 | Session kept after a restart | `quit`, start again: no QR code, `CONNECTED` | | | | |
| 4 | W01 | Reconnect after network loss | Turn Wi-Fi off for a minute, then on: `DISCONNECTED`, then `CONNECTED` | | | | |
| 5 | W06 | History sync on first link | After linking: `HISTORY` lines; `stats` shows conversations and messages | | | | |
| 6 | W05 | Receive text, direct and group | Send from the other phone: `MSG` line with the right chat and sender | | | | |
| 7 | W07 | Captions | Send a photo with a caption: `image caption=` is not empty | | | | |
| 8 | W08 | Non-text messages | Send a reaction, poll, location, contact card, sticker; edit and delete a message: each gives a `MSG` line of the right kind | | | | |
| 9 | W09 | Quoted message ID | Reply to a message on the phone: `quotes=<id>` | | | | |
| 10 | W10 | LID-to-phone map | For a LID sender: `lid=… pn=…` on the `MSG` line, or `lid <jid>` | | | | |
| 11 | W10 | Contact and push names | `push_name` on `MSG` lines (with `--show-content`); `CONTACT` lines; `stats` | | | | |
| 12 | W11 | Group names | `group <jid>` shows the subject (with `--show-content`) and member count; `groups` lists them | | | | |
| 13 | W13 | Send text | `send <your number> hello` arrives on the phone | | | | |
| 14 | W13 | Reply | `reply <id> text` shows as a reply on the phone, also in the group | | | | |
| 15 | W13 | Mention | `mention <group> <number> @<number> hi` highlights the mention | | | | |
| 16 | W14 | Send image, video, document, voice note | Each arrives and opens; the document keeps its name and type | | | | |
| 17 | W15 | React and remove a reaction | `react <id> 👍`, then `unreact <id>`, direct and in the group | | | | |
| 18 | W16 | Download media | `download <id>` for a photo, video, voice note and document from the phone | | | | |
| 19 | W17 | Re-request expired media | `download` on an old photo from history fails; `retry <id>` with the phone online succeeds | | | | |
| 20 | — | Builds without warnings | `cargo build --release` | | | | |

## Results

Not run yet.

## Open items

- **Storing into `messages.db`** is not part of this test. That is phase 5; this phase only checks that the library gives us everything the bridge stores.
