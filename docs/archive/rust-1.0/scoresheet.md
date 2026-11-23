# 1.0 score sheet

Feature parity between today's code (Go bridge + Python MCP server) and the Rust rewrite. See the [plan](plan.md) and [the interface to keep](interface.md).

**How to keep it up to date:** every pull request for the rewrite updates the rows it touches, and the totals below. A row moves forward only when its check is in place.

| Status | Meaning |
|---|---|
| `todo` | Not started. |
| `wip` | Being built. |
| `done` | Built in Rust; its tests (ported or new) pass in CI. |
| `verified` | `done`, and it passes the parity harness (or for bridge rows, works in a live test with the test account). Also marked by whom and when. |
| `changed` | Intentionally different from today; the Notes column says how and why. |
| `dropped` | Not carried over; the Notes column says why. |

The **Check** column says how parity is shown:
- `harness`: the parity harness compares golden output.
- `tests`: today's tests are ported.
- `live`: a manual test with the WhatsApp test account (a spare number, not a personal one).
- `CI`: a CI job or check that must pass, such as a build, lint or audit step.
- `review`: checked by reading, in the pull request that makes the change (for documentation and clean-up).

## Totals

Last updated: 2026-11-15 (v1.0.0 tagged: R03 and R08 done).

| Area | Rows | todo | wip | done | verified | changed / dropped |
|---|---|---|---|---|---|---|
| MCP interface | 20 | 0 | 0 | 0 | 18 | 2 |
| Configuration | 16 | 0 | 0 | 0 | 15 | 1 |
| Bridge: WhatsApp | 17 | 0 | 1 | 0 | 16 | 0 |
| Bridge: API and security | 8 | 0 | 0 | 0 | 8 | 0 |
| Bridge: storage | 6 | 0 | 0 | 0 | 6 | 0 |
| Build, release, docs | 10 | 0 | 0 | 2 | 8 | 0 |
| **Total** | **77** | **0** | **1** | **2** | **71** | **3** |

## MCP interface

What Claude and other MCP clients see. Today: `whatsapp-mcp-server/main.py` and `whatsapp.py`.

| ID | Feature | Check | Status | Notes |
|---|---|---|---|---|
| M01 | Server name `whatsapp` and the server instructions text | harness | verified | Harness: `_server.json` in every profile matches (`run.py --server rust --only _server`). Verified 2026-11-15 (CI on `dev-main` at 86b5be4): the parity harness passes. |
| M02 | Tool list: names, titles, descriptions, parameter schemas and defaults | harness | verified | Definitions are data (`crates/mcp/src/tools.json`); `crates/mcp/tests/definitions.rs` keeps them equal to the golden file. Verified 2026-11-15 (CI on `dev-main` at 86b5be4): the parity harness passes. |
| M03 | Tool annotations (read-only, destructive, idempotent, open-world) | harness | verified | Part of the definitions; checked as M02. Verified 2026-11-15 (CI on `dev-main` at 86b5be4): the parity harness passes. |
| M04 | Read-only mode hides the send tools | harness | changed | 1.0 ([#44](https://github.com/erwinvaneijk/whatsapp-mcp/issues/44)): read-only is the default; `full_control` / `WHATSAPP_FULL_CONTROL` offers the send tools, and the old `read_only` still works. Harness profiles `read_only_default`, `full_control_config`, `read_only_env`, `read_only_config` and `read_only_no_config`; `definitions.rs` checks exactly the four send tools are hidden without full control. Before: verified 2026-11-15 at 86b5be4. |
| M05 | `search_contacts` | harness | verified | Names and LID-to-phone map; see plan, "Contact names". Harness: Rust matches the golden files (phase 4, step 2). Verified 2026-11-15 (CI on `dev-main` at 86b5be4): the parity harness passes. |
| M06 | `list_messages`: filters (time, sender, chat), paging, sorting | harness | verified | Harness: Rust matches the golden files (phase 4, step 2). Time filters also in Amsterdam and New York, and across the DST change. Verified 2026-11-15 (CI on `dev-main` at 86b5be4): the parity harness passes. |
| M07 | `list_messages`: text search, FTS5 and the fallback scan | harness | verified | Rust always has FTS5; the fallback is still needed for a database without the index. Harness: Rust matches the golden files (phase 4, step 2). Both with the FTS5 index (`default`) and without (`no_fts`). Verified 2026-11-15 (CI on `dev-main` at 86b5be4): the parity harness passes. |
| M08 | `list_messages`: context around each match | harness | verified | Harness: Rust matches the golden files (phase 4, step 2). Verified 2026-11-15 (CI on `dev-main` at 86b5be4): the parity harness passes. |
| M09 | `list_chats`: search, paging, sort, with and without last message | harness | verified | Harness: Rust matches the golden files (phase 4, step 2). Verified 2026-11-15 (CI on `dev-main` at 86b5be4): the parity harness passes. |
| M10 | `get_chat`, `get_direct_chat_by_contact`, `get_contact_chats` | harness | verified | Harness: Rust matches the golden files (phase 4, step 2). Verified 2026-11-15 (CI on `dev-main` at 86b5be4): the parity harness passes. |
| M11 | `get_last_interaction`, `get_message_context` | harness | verified | Harness: Rust matches the golden files (phase 4, step 2). Verified 2026-11-15 (CI on `dev-main` at 86b5be4): the parity harness passes. |
| M12 | `send_message`: text, reply, mentions | harness | verified | Against a fake bridge that records requests. Harness: Rust matches the golden files (phase 4, step 3). Verified 2026-11-15 (CI on `dev-main` at 86b5be4): the parity harness passes. |
| M13 | `send_file`: caption, only from `media_dir` | harness | verified | Harness: Rust matches the golden files (phase 4, step 3). Media folder checks also unit-tested (`..`, symlinks, shared prefix). Verified 2026-11-15 (CI on `dev-main` at 86b5be4): the parity harness passes. |
| M14 | `send_audio_message`: ffmpeg conversion, temp file cleanup | harness, tests | verified | Harness: Rust matches the golden files (phase 4, step 3). Only `.ogg` in the harness; the ffmpeg conversion and temp file cleanup are unit-tested (CI installs ffmpeg on Linux x86_64). Verified 2026-11-15 (CI on `dev-main` at 86b5be4): the parity harness passes. |
| M15 | `download_media`: returns the file path | harness | verified | Harness: Rust matches the golden files (phase 4, step 3). Verified 2026-11-15 (CI on `dev-main` at 86b5be4): the parity harness passes. |
| M16 | `react_to_message`, including removing a reaction | harness | verified | Harness: Rust matches the golden files (phase 4, step 3). Verified 2026-11-15 (CI on `dev-main` at 86b5be4): the parity harness passes. |
| M17 | Structured output: same JSON fields and types as today | harness | verified | The Python MCP SDK (`mcp` 2.x) validates return types against the declared output schema; the Rust server must declare and return the same schemas. All tools: text and structured content match (step 3). Verified 2026-11-15 (CI on `dev-main` at 86b5be4): the parity harness passes. |
| M18 | Messages from others marked as data (`<message_content>`, tag neutralising, names on one line, read-tool notice) | harness, tests | verified | Harness: Rust matches the golden files (phase 4, step 2). Unit tests in `crates/mcp/src/untrusted.rs`. Verified 2026-11-15 (CI on `dev-main` at 86b5be4): the parity harness passes. |
| M19 | Times shown with UTC offset; `after`/`before` accept ISO-8601, naive times are local | harness, tests | verified | Harness: Rust matches the golden files (phase 4, step 2). Unit tests in `crates/store/src/time.rs`. Verified 2026-11-15 (CI on `dev-main` at 86b5be4): the parity harness passes. |
| M20 | Error texts for bad input and an unreachable bridge | harness | changed | Decided: today's quirks get better behaviour in Rust, so this row becomes `changed` when done; see [interface.md, section 5](interface.md#5-behaviour-that-improves-in-10). Eleven cases have Rust-specific golden files (`*.rust.json`): bad input and bridge errors give their reason. Bridge-down and missing-token messages are unchanged. |

## Configuration

Same file, same environment variables, same rules. Today: `whatsapp-bridge/config.go` and `whatsapp-mcp-server/config.py`.

| ID | Feature | Check | Status | Notes |
|---|---|---|---|---|
| C01 | File at `~/.config/whatsapp-mcp/config.yaml`, or the path in `WHATSAPP_MCP_CONFIG` (must exist when named) | tests | verified | `whatsapp-mcp-config`: `Config::load`; tests in `crates/config/tests/load.rs`. `~/` at the start of `WHATSAPP_MCP_CONFIG` means the home folder, as in Python. Verified 2026-11-15: the shared cases (`parity/config-cases.json`) pass in Go and Rust in CI, and the week of use (user, 2026-11-08 to 2026-11-15) ran on the configuration file. |
| C02 | A set environment variable wins; an empty one counts as unset | tests | verified | `load.rs`: `environment_wins_and_empty_counts_as_unset`. Verified 2026-11-15: the shared cases (`parity/config-cases.json`) pass in Go and Rust in CI, and the week of use (user, 2026-11-08 to 2026-11-15) ran on the configuration file. |
| C03 | Values keep their exact text (`0612` stays `0612`) | tests | verified | Works on the YAML parser's events, so values keep their text; shared cases in `parity/config-cases.json`. Verified 2026-11-15: the shared cases (`parity/config-cases.json`) pass in Go and Rust in CI, and the week of use (user, 2026-11-08 to 2026-11-15) ran on the configuration file. |
| C04 | `send_allowlist` as a YAML list or a separated string | tests | verified | Shared cases: block list, flow list, empty list, one line. Verified 2026-11-15: the shared cases (`parity/config-cases.json`) pass in Go and Rust in CI, and the week of use (user, 2026-11-08 to 2026-11-15) ran on the configuration file. |
| C05 | Unknown setting or wrong type is an error with the line number | tests | verified | Shared cases check the line number and the name. Stricter than today: a setting given twice, several YAML documents, and YAML aliases are errors too. Verified 2026-11-15: the shared cases (`parity/config-cases.json`) pass in Go and Rust in CI, and the week of use (user, 2026-11-08 to 2026-11-15) ran on the configuration file. |
| C06 | Refuse a file others can read or change (not on Windows) | tests | verified | `load.rs`: modes 644, 620 and 604 are refused; not checked on Windows. Verified 2026-11-15: the shared cases (`parity/config-cases.json`) pass in Go and Rust in CI, and the week of use (user, 2026-11-08 to 2026-11-15) ran on the configuration file. |
| C07 | `bridge_token` / `WHATSAPP_BRIDGE_TOKEN`: bridge refuses to start without one (32+ characters) | tests | verified | `Config::bridge_token` refuses fewer than 32 characters; the bridge stops with that message, after its startup block shows `too short`. Phase 5, step 5: shown in the startup block of both commands, never the token itself (tests). Verified 2026-11-15: the shared cases (`parity/config-cases.json`) pass in Go and Rust in CI, and the week of use (user, 2026-11-08 to 2026-11-15) ran on the configuration file. |
| C08 | `media_dir` / `WHATSAPP_MEDIA_DIR` | tests | verified | `Config::media_dir`: `send_file` in `serve` and sending in the bridge (phase 5, step 4). Phase 5, step 5: shown in the startup block of both commands. Verified 2026-11-15: the shared cases (`parity/config-cases.json`) pass in Go and Rust in CI, and the week of use (user, 2026-11-08 to 2026-11-15) ran on the configuration file. |
| C09 | `send_allowlist` / `WHATSAPP_SEND_ALLOWLIST`: a bad entry stops the bridge | tests | verified | `SendAllowlist::parse`, with the bridge's rules; shared cases run against Go and Rust. A bad entry stops the bridge; a LID matches its phone number through `lid_map` (phase 5, step 2). Phase 5, step 5: shown in the startup block of both commands, as a count. Verified 2026-11-15: the shared cases (`parity/config-cases.json`) pass in Go and Rust in CI, and the week of use (user, 2026-11-08 to 2026-11-15) ran on the configuration file. |
| C10 | `read_only` / `WHATSAPP_MCP_READ_ONLY` | tests | changed | 1.0 ([#44](https://github.com/erwinvaneijk/whatsapp-mcp/issues/44)): replaced by `full_control` / `WHATSAPP_FULL_CONTROL`, off by default, for `serve` and `bridge` (403 on send and react). `read_only` still counts as its opposite until 2.0, with a warning; the two disagreeing is an error (`load.rs`, shared cases). Both programs log it on start. Before: verified 2026-11-15. |
| C11 | `log_content` / `WHATSAPP_LOG_CONTENT` | tests | verified | `Config::log_content`, used by the bridge since phase 5, step 2. Phase 5, step 5: shown in the startup block of both commands. Verified 2026-11-15: the shared cases (`parity/config-cases.json`) pass in Go and Rust in CI, and the week of use (user, 2026-11-08 to 2026-11-15) ran on the configuration file. |
| C12 | `pair_phone` / `WHATSAPP_PAIR_PHONE` | tests | verified | `Config::pair_phone`, used by the bridge since phase 5, step 3 (verified live). Phase 5, step 5: shown in the startup block of the bridge, as set or not set only. Verified 2026-11-15: the shared cases (`parity/config-cases.json`) pass in Go and Rust in CI, and the week of use (user, 2026-11-08 to 2026-11-15) ran on the configuration file. |
| C13 | `log_output` / `WHATSAPP_LOG_OUTPUT`: `stderr`, `stdout`, `syslog` or a file; never stdout for `serve` | tests | verified | New in Rust; phase 5, step 6: `Config::log_output`, `whatsapp-mcp-logging` (tests for stderr, a file and a syslog socket; `serve` refuses stdout). The Go bridge accepts and ignores it (shared case in `parity/config-cases.json`). Verified live (Claude, 2026-11-15) with `serve` on macOS: a log file is created `0600`, `json` gives one object per line, `warn` leaves out the info lines, an unknown level is refused; syslog reaches macOS's unified log (`log show`) and, in CI, Linux's journal. |
| C14 | `log_format` / `WHATSAPP_LOG_FORMAT`: `text` or `json` | tests | verified | New in Rust; phase 5, step 6: `Config::log_format`; text and JSON lines tested. Verified live (Claude, 2026-11-15) with `serve` on macOS: a log file is created `0600`, `json` gives one object per line, `warn` leaves out the info lines, an unknown level is refused; syslog reaches macOS's unified log (`log show`) and, in CI, Linux's journal. |
| C15 | `log_level` / `WHATSAPP_LOG_LEVEL`: `error`, `warn`, `info`, `debug` | tests | verified | New in Rust; phase 5, step 6: `Config::log_level`; our crates from the level, libraries from `warn`, `RUST_LOG` on top per module (tests). Verified live (Claude, 2026-11-15) with `serve` on macOS: a log file is created `0600`, `json` gives one object per line, `warn` leaves out the info lines, an unknown level is refused; syslog reaches macOS's unified log (`log show`) and, in CI, Linux's journal. |
| C16 | `store_dir` / `WHATSAPP_STORE_DIR`: the store folder, absolute or `~/`; `--store` overrides it | tests | verified | New in 1.0 ([#50](https://github.com/erwinvaneijk/whatsapp-mcp/issues/50)): `Config::store_dir`, used by `bridge`, `serve`, `link` and `service install`; the startup log says where the store comes from. Tests: `load.rs` (`~/`, relative refused), `main.rs` (flag, setting, default), shared cases. Verified 2026-11-15: CI on `dev-main` (8a19dac) passes, Windows included. |

## Bridge: WhatsApp

Today: `whatsapp-bridge/main.go` and friends, on whatsmeow. Library rows are tested first in the phase 1 go/no-go test; its results are in [phase1.md](phase1.md).

| ID | Feature | Check | Status | Notes |
|---|---|---|---|---|
| W01 | Connect and stay connected; reconnect after network loss | live | verified | Live test passed with the test account (user, 2026-11-06), phase 5 step 3, check 4: reconnects after 2 minutes offline, the message sent meanwhile is stored. |
| W02 | Link with a QR code in the terminal | live | verified | Live test passed with the test account (user, 2026-11-06), phase 5 step 3, check 1. |
| W03 | Link with a pairing code (`pair_phone`) | live | verified | Live test passed with the test account (user, 2026-11-06), phase 5 step 3, check 2 (`WHATSAPP_PAIR_PHONE`). The number must include the country code; a leading 0 is refused. |
| W04 | Session kept across restarts | live | verified | Live test passed with the test account (user, 2026-11-06), phase 5 step 3, check 3. The session is in `store/session.db`; the Go bridge's link cannot be carried over (re-linked once). |
| W05 | Receive and store text messages, direct and group | tests, live | verified | Live test passed with the test account (user, 2026-11-06), phase 5 step 3, check 6, also messages sent from the phone itself. Tests: `crates/bridge/tests/receive.rs`. |
| W06 | History sync on first link | live | verified | Live test passed with the test account (user, 2026-11-06), phase 5 step 3, check 5. Go's rules (senders, skipped conversations and messages) tested in `receive.rs`. |
| W07 | Captions stored as message text | tests, live | verified | Tests ported (`caption_test.go`). Live test passed with the test account (user, 2026-11-06), phase 5 step 3, check 7. |
| W08 | Non-text messages stored as bracketed text (reactions, polls, votes, locations, contacts, stickers, edits, deletes) | tests, live | verified | Tests ported (`reaction_test.go`, content parts of `main_test.go`). Live test passed with the test account (user, 2026-11-06), phase 5 step 3, check 8. |
| W09 | Quoted message ID stored for replies | tests, live | verified | Tests ported (`quoted_test.go`). Live test passed with the test account (user, 2026-11-06), phase 5 step 3, check 9. |
| W10 | Contact names, push names, LID-to-phone map | tests, live | verified | Stored in `contacts` and `lid_map` in `messages.db` (see plan), read by `serve`. Live test passed with the test account (user, 2026-11-06), phase 5 step 3, checks 10–11. |
| W11 | Group names | live | verified | Live test passed with the test account (user, 2026-11-06), phase 5 step 3, check 12: the group's subject, asked from WhatsApp when the history sync has no name. |
| W12 | Sent messages and reactions stored, in the existing PN or LID chat | tests | verified | Phase 5, step 4: stored from the message as sent, like a received one, in the chat that exists for the phone number or LID (`crates/bridge/tests/send.rs`, ported from `outgoing_test.go`). Week of use passed (user, 2026-11-08 to 2026-11-15): the Rust bridge in daily use on the user's own `messages.db`. Sent messages show up in their chat. |
| W13 | Send text with reply and mentions | live | verified | Phase 5, step 4: built; replies and mentions tested (ported from `send_context_test.go`). Live test passed with the test account (user, 2026-11-07), phase 5 step 4, checks 13–15, through the MCP tools. |
| W14 | Send image, video, document (type and name), voice note | live | verified | Phase 5, step 4: built; MIME types, media messages and the voice note length and waveform tested. Live test passed with the test account (user, 2026-11-07), phase 5 step 4, check 16. |
| W15 | Send a reaction, and remove one | live | verified | Phase 5, step 4: built; the original sender from `messages.db` (ported from `reaction_test.go`). Live test passed with the test account (user, 2026-11-07), phase 5 step 4, check 17. |
| W16 | Download media, signed URL kept, file inside `store/` | tests, live | verified | Phase 5, step 4: built; the signed path kept byte for byte, safe file names, one file per message (ported from `media_url_test.go`, `media_download_test.go`). Live test passed with the test account (user, 2026-11-07), phase 5 step 4, check 18. |
| W17 | Expired media requested again from the sender's phone | live | wip | Phase 5, step 4: built on whatsapp-rust's media re-upload request; the message to ask for tested (ported from `media_retry_test.go`). Live test open (check 19): needs media that has expired on WhatsApp's servers. Does not block 1.0 (user, 2026-11-15); listed as not tested live in the release notes. |

## Bridge: API and security

| ID | Feature | Check | Status | Notes |
|---|---|---|---|---|
| A01 | API on `127.0.0.1:8080` only: `/api/send`, `/api/react`, `/api/download` with the same requests and responses | tests | verified | Internal, but kept so the Go bridge and Rust server mix during the switch. Phase 5, step 2: `crates/bridge/src/api.rs`, tested row by row against interface.md section 3 (`crates/bridge/tests/api.rs`, also over a real socket). The bridge serves it from step 3. Phase 5, step 4: the bridge's backend sends, reacts and downloads; the answers when not connected match Go's. Week of use passed (user, 2026-11-08 to 2026-11-15): the Rust bridge in daily use on the user's own `messages.db`. `serve` sends through this API. |
| A02 | `X-Bridge-Token` required, compared in constant time | tests | verified | Phase 5, step 2: checked before anything else, compared in constant time (tests). Week of use passed (user, 2026-11-08 to 2026-11-15): the Rust bridge in daily use on the user's own `messages.db`. Every send carried the token. |
| A03 | Files only sent from `media_dir`; downloads stay inside `store/` | tests | verified | Phase 5, step 2: `allowed_media_path` with the Go bridge's rules and messages (tests, symlinks included). Downloads into `store/` come in step 4. Phase 5, step 4: downloads go into `<store>/<chat>/` with the chat and file names reduced to one safe path component (tests). Live test passed with the test account (user, 2026-11-07), phase 5 step 4: a file outside the media folder was refused; downloads landed in `store/<chat>/`. |
| A04 | Send allowlist by phone number or LID, refused with 403 | tests | verified | Phase 5, step 2: 403 before sending, matching by number or LID (tests ported from `allowlist_test.go`). The bridge fills the LID map since step 3; sending comes in step 4. Phase 5, step 4: checked before every send and reaction. Live test passed with the test account (user, 2026-11-07), phase 5 step 4: a chat outside the allowlist was refused, and the MCP client showed the reason and what to change. |
| A05 | `store/` and its files readable by the user only | tests | verified | Phase 5: `messages.db` and its WAL files (step 1), the session in `session.db` with its WAL files (step 3), chat folders for downloads `0700` and downloaded files `0600` (step 4); step 5 ports the rest of `fsperm_test.go` (an existing world-readable folder or file is made private and keeps its content) and tests the session and downloads. Verified after the week of use (Claude, 2026-11-15): in the user's store the folder and chat folders are `0700`, `messages.db` and `session.db` `0600`, nothing readable by group or others. |
| A06 | Message content out of logs unless `log_content`; media keys never logged | tests | verified | Phase 5: the API's log lines as in Go, content only with `log_content` (`log_test.go` ported, step 2). Step 5: a review of every log line; control characters are removed from every value and error text, and from each whole line; upload results and media keys are never logged, and the library's own media errors carry only status codes. The library's warnings (stderr) name message IDs and JIDs, not content; `RUST_LOG=debug` logs more and is not for shared logs. Verified live (Claude, 2026-11-15) in the user's bridge log file, with `log_content` off: a received message logs only `← message stored`, without text, sender or file name. |
| A07 | No `unsafe` in our crates; `cargo audit` and `cargo deny` in CI | CI | verified | New in Rust. Since phase 2: `unsafe_code = "forbid"`, and `cargo deny` (which covers the RustSec database `cargo audit` uses) in CI. Verified 2026-11-15: `cargo deny check` passes in CI on `dev-main` at 86b5be4, after the SQLite fix for CVE-2026-119375 (#32). |
| A08 | Every log line has a time (local, with offset), a level and its source, for `serve`, `bridge` and the library | tests | verified | New in Rust; phase 5, step 6: every line through the `log` facade and `whatsapp-mcp-logging` (replaces `env_logger`), with local time, offset and milliseconds, level and source; control characters removed from each line. QR and pairing codes only on the terminal. Verified live (user, 2026-11-15): the bridge on the current build, logging to a file. |

## Bridge: storage

`store/messages.db` must work unchanged.

| ID | Feature | Check | Status | Notes |
|---|---|---|---|---|
| S01 | Same `chats` and `messages` schema; opens an existing database without loss | tests | verified | Tested on a copy of a real database. Phase 5, step 1: the Rust schema is the Go bridge's, byte for byte (`crates/store/tests/write.rs` compares it with `parity/fixture/schema.sql`). The test on a copy of a real database is still to do. Week of use passed (user, 2026-11-08 to 2026-11-15): the Rust bridge in daily use on the user's own `messages.db`. It opened the database the Go bridge wrote, with all history (user, 2026-11-15). |
| S02 | Same migrations (e.g. `quoted_message_id`), safe to run repeatedly | tests | verified | Phase 5, step 1: `quoted_message_id` added to older databases; opening twice changes nothing (tests). Verified after the week of use (Claude, 2026-11-15), read-only on the user's `messages.db`: `quoted_message_id` is there. |
| S03 | FTS5 table and triggers | tests | verified | Phase 5, step 1: always created (bundled FTS5), rebuilt on open; leftover triggers do not break inserts (test ported from `fts_test.go`). Verified after the week of use (Claude, 2026-11-15), read-only on the user's `messages.db`: the `messages_fts` index and its tables are there. |
| S04 | Indexes, including the `julianday()` time indexes | tests | verified | Phase 5, step 1: old time indexes replaced by `julianday()` ones (test ported from `moment_index_test.go`). Verified after the week of use (Claude, 2026-11-15), read-only on the user's `messages.db`: the four `julianday()` indexes are there. |
| S05 | WAL mode | tests | verified | Phase 5, step 1: also foreign keys, normal sync and a 5 s busy wait, as Go (test). Verified after the week of use (Claude, 2026-11-15), read-only on the user's `messages.db`: the journal mode is `wal`. |
| S06 | Last message per chat chosen by row, not timestamp | tests | verified | The read queries join the latest message by rowid (phase 4); the parity harness checks it. Verified 2026-11-15 (CI on `dev-main` at 86b5be4): the parity harness passes. |

## Build, release, docs

| ID | Feature | Check | Status | Notes |
|---|---|---|---|---|
| R01 | Parity harness and fixture database, passing against today's code | CI | verified | Phase 0: [`parity/`](../../../parity/README.md), 93 golden files, `Parity harness` CI job. Verified 2026-11-15 (CI on `dev-main` at 86b5be4): the parity harness passes. |
| R02 | CI: rustfmt, clippy, tests on macOS, Linux, Windows | CI | verified | Phase 2: `Rust (…)` CI jobs on Linux x86_64 and arm64, macOS arm64 and x86_64, Windows x86_64 (macOS arm64 in a workflow of its own, `macos-arm64.yml`, because GitHub often has no runner for it); see [crates/README.md](../../../crates/README.md). Verified 2026-11-15: all five CI jobs pass on `dev-main` at 86b5be4. |
| R03 | Release binaries: macOS arm64 and x86_64, Linux x86_64 and arm64, Windows x86_64, with checksums | CI | done | Phase 6: [`release.yml`](../../../.github/workflows/release.yml) builds all five on their own runners (Linux on Ubuntu 22.04, for glibc 2.35 and newer) and, on a `v*` tag, attaches them and `SHA256SUMS` to a draft release. Done 2026-11-15: `v1.0.0` built all five and `SHA256SUMS` into a draft release; the macOS arm64 binary checked against `SHA256SUMS` and run (`whatsapp-mcp 1.0.0`, system libraries only). |
| R04 | Migration guide: client config, re-pairing, keeping `messages.db` | live | verified | Phase 6: [Upgrading to 1.0](../../upgrading-to-1.0.md). The user's switch from the Go bridge for the week of use counts as the run (user, 2026-11-15). |
| R05 | README rewritten for the single binary | review | verified | 0.9: installation, client configuration and an upgrade section rewritten for the Rust MCP server. 1.0: installation from a release or with `cargo`, `bridge` and `serve`, without Go; `main` is the release branch. Reviewed by the user, 2026-11-15. |
| R06 | `SECURITY.md` and the issue page updated | review | verified | 1.0: SECURITY.md covers the latest release and `whatsapp-mcp` (bridge and serve); issue templates in `.github/ISSUE_TEMPLATE/`. Issues are turned off on the repository; they show once turned on. Reviewed by the user, 2026-11-15. |
| R07 | Go and Python code removed | review | verified | 0.9: the Python server is removed. 1.0: the Go bridge and its CI job are removed; the parity harness (Python, test tooling only) stays. Reviewed by the user, 2026-11-15. |
| R08 | `dev-main` merged into `main`, tagged `v1.0.0` | review | done | Approved by the user, 2026-11-15. `main` fast-forwarded to 733e7c6 and tagged `v1.0.0`. The draft release waits for the user to publish it. |
| R09 | Dependency checks (vulnerabilities, licenses, sources) and no `unsafe` code | CI | verified | Phase 2: `cargo deny check` in the `Rust dependencies` CI job ([deny.toml](../../../deny.toml)); `unsafe_code = "forbid"` for every crate. Verified 2026-11-15: `cargo deny check` passes in CI on `dev-main` at 86b5be4. |
| R10 | Link, then run the bridge as a service: `link` (`login`), `service install`, `status`, `uninstall` | live | verified | [#45](https://github.com/erwinvaneijk/whatsapp-mcp/issues/45). Verified live on macOS by the user, 2026-11-15: `store_dir` set, `link`, `service install`, log out and in, `status`, `uninstall`. Linux: by the tests of the unit file only; noted in the 1.0 release notes. |
