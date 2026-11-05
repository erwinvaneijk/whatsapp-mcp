# 1.0 score sheet

Feature parity between today's code (Go bridge + Python MCP server) and the Rust rewrite. See the [plan](plan.md).

**How to keep it up to date:** every pull request for the rewrite updates the rows it touches, and the totals below. A row moves forward only when its check is in place.

| Status | Meaning |
|---|---|
| `todo` | Not started. |
| `wip` | Being built. |
| `done` | Built in Rust; its tests (ported or new) pass in CI. |
| `verified` | `done`, and it passes the parity harness (or for bridge rows, works in a live test with a real account). Also marked by whom and when. |
| `changed` | Intentionally different from today; the Notes column says how and why. |
| `dropped` | Not carried over; the Notes column says why. |

The **Check** column says how parity is shown:
- `harness`: the parity harness compares golden output.
- `tests`: today's tests are ported.
- `live`: a manual test with a real account.

## Totals

Last updated: 2026-11-05 (plan written, no Rust code yet).

| Area | Rows | todo | wip | done | verified | changed / dropped |
|---|---|---|---|---|---|---|
| MCP interface | 20 | 20 | 0 | 0 | 0 | 0 |
| Configuration | 12 | 12 | 0 | 0 | 0 | 0 |
| Bridge: WhatsApp | 17 | 17 | 0 | 0 | 0 | 0 |
| Bridge: API and security | 7 | 7 | 0 | 0 | 0 | 0 |
| Bridge: storage | 6 | 6 | 0 | 0 | 0 | 0 |
| Build, release, docs | 8 | 8 | 0 | 0 | 0 | 0 |
| **Total** | **70** | **70** | **0** | **0** | **0** | **0** |

## MCP interface

What Claude and other MCP clients see. Today: `whatsapp-mcp-server/main.py` and `whatsapp.py`.

| ID | Feature | Check | Status | Notes |
|---|---|---|---|---|
| M01 | Server name `whatsapp` and the server instructions text | harness | todo | |
| M02 | Tool list: names, titles, descriptions, parameter schemas and defaults | harness | todo | |
| M03 | Tool annotations (read-only, destructive, idempotent, open-world) | harness | todo | |
| M04 | Read-only mode hides the send tools | harness | todo | `read_only` / `WHATSAPP_MCP_READ_ONLY` |
| M05 | `search_contacts` | harness | todo | Names and LID-to-phone map; see plan, "Contact names". |
| M06 | `list_messages`: filters (time, sender, chat), paging, sorting | harness | todo | |
| M07 | `list_messages`: text search, FTS5 and the fallback scan | harness | todo | Rust always has FTS5; the fallback is still needed for a database without the index. |
| M08 | `list_messages`: context around each match | harness | todo | |
| M09 | `list_chats`: search, paging, sort, with and without last message | harness | todo | |
| M10 | `get_chat`, `get_direct_chat_by_contact`, `get_contact_chats` | harness | todo | |
| M11 | `get_last_interaction`, `get_message_context` | harness | todo | |
| M12 | `send_message`: text, reply, mentions | harness | todo | Against a fake bridge that records requests. |
| M13 | `send_file`: caption, only from `media_dir` | harness | todo | |
| M14 | `send_audio_message`: ffmpeg conversion, temp file cleanup | harness, tests | todo | |
| M15 | `download_media`: returns the file path | harness | todo | |
| M16 | `react_to_message`, including removing a reaction | harness | todo | |
| M17 | Structured output: same JSON fields and types as today | harness | todo | mcp 2.x validates return types; Rust must produce the same schemas. |
| M18 | Messages from others marked as data (`<message_content>`, tag neutralising, names on one line, read-tool notice) | harness, tests | todo | |
| M19 | Times shown with UTC offset; `after`/`before` accept ISO-8601, naive times are local | harness, tests | todo | |
| M20 | Error texts for bad input and an unreachable bridge | harness | todo | |

## Configuration

Same file, same environment variables, same rules. Today: `whatsapp-bridge/config.go` and `whatsapp-mcp-server/config.py`.

| ID | Feature | Check | Status | Notes |
|---|---|---|---|---|
| C01 | File at `~/.config/whatsapp-mcp/config.yaml`, or the path in `WHATSAPP_MCP_CONFIG` (must exist when named) | tests | todo | |
| C02 | A set environment variable wins; an empty one counts as unset | tests | todo | |
| C03 | Values keep their exact text (`0612` stays `0612`) | tests | todo | |
| C04 | `send_allowlist` as a YAML list or a separated string | tests | todo | |
| C05 | Unknown setting or wrong type is an error with the line number | tests | todo | |
| C06 | Refuse a file others can read or change (not on Windows) | tests | todo | |
| C07 | `bridge_token` / `WHATSAPP_BRIDGE_TOKEN`: bridge refuses to start without one (32+ characters) | tests | todo | |
| C08 | `media_dir` / `WHATSAPP_MEDIA_DIR` | tests | todo | |
| C09 | `send_allowlist` / `WHATSAPP_SEND_ALLOWLIST`: a bad entry stops the bridge | tests | todo | |
| C10 | `read_only` / `WHATSAPP_MCP_READ_ONLY` | tests | todo | |
| C11 | `log_content` / `WHATSAPP_LOG_CONTENT` | tests | todo | |
| C12 | `pair_phone` / `WHATSAPP_PAIR_PHONE` | tests | todo | |

## Bridge: WhatsApp

Today: `whatsapp-bridge/main.go` and friends, on whatsmeow. Library rows are tested first in the phase 1 go/no-go test.

| ID | Feature | Check | Status | Notes |
|---|---|---|---|---|
| W01 | Connect and stay connected; reconnect after network loss | live | todo | |
| W02 | Link with a QR code in the terminal | live | todo | |
| W03 | Link with a pairing code (`pair_phone`) | live | todo | |
| W04 | Session kept across restarts | live | todo | Existing whatsmeow session probably cannot be carried over; see plan. |
| W05 | Receive and store text messages, direct and group | tests, live | todo | |
| W06 | History sync on first link | live | todo | |
| W07 | Captions stored as message text | tests | todo | |
| W08 | Non-text messages stored as bracketed text (reactions, polls, votes, locations, contacts, stickers, edits, deletes) | tests | todo | |
| W09 | Quoted message ID stored for replies | tests | todo | |
| W10 | Contact names, push names, LID-to-phone map | tests, live | todo | Copied into `messages.db`; see plan. |
| W11 | Group names | live | todo | |
| W12 | Sent messages and reactions stored, in the existing PN or LID chat | tests | todo | |
| W13 | Send text with reply and mentions | live | todo | |
| W14 | Send image, video, document (type and name), voice note | live | todo | |
| W15 | Send a reaction, and remove one | live | todo | |
| W16 | Download media, signed URL kept, file inside `store/` | tests, live | todo | |
| W17 | Expired media requested again from the sender's phone | live | todo | |

## Bridge: API and security

| ID | Feature | Check | Status | Notes |
|---|---|---|---|---|
| A01 | API on `127.0.0.1:8080` only: `/api/send`, `/api/react`, `/api/download` with the same requests and responses | tests | todo | Internal, but kept so the Go bridge and Rust server mix during the switch. |
| A02 | `X-Bridge-Token` required, compared in constant time | tests | todo | |
| A03 | Files only sent from `media_dir`; downloads stay inside `store/` | tests | todo | |
| A04 | Send allowlist by phone number or LID, refused with 403 | tests | todo | |
| A05 | `store/` and its files readable by the user only | tests | todo | |
| A06 | Message content out of logs unless `log_content`; media keys never logged | tests | todo | |
| A07 | No `unsafe` in our crates; `cargo audit` and `cargo deny` in CI | CI | todo | New in Rust. |

## Bridge: storage

`store/messages.db` must work unchanged.

| ID | Feature | Check | Status | Notes |
|---|---|---|---|---|
| S01 | Same `chats` and `messages` schema; opens an existing database without loss | tests | todo | Tested on a copy of a real database. |
| S02 | Same migrations (e.g. `quoted_message_id`), safe to run repeatedly | tests | todo | |
| S03 | FTS5 table and triggers | tests | todo | |
| S04 | Indexes, including the `julianday()` time indexes | tests | todo | |
| S05 | WAL mode | tests | todo | |
| S06 | Last message per chat chosen by row, not timestamp | tests | todo | |

## Build, release, docs

| ID | Feature | Check | Status | Notes |
|---|---|---|---|---|
| R01 | Parity harness and fixture database, passing against today's code | CI | todo | Phase 0. |
| R02 | CI: rustfmt, clippy, tests on macOS, Linux, Windows | CI | todo | |
| R03 | Release binaries: macOS arm64 and x86_64, Linux x86_64 and arm64, Windows x86_64, with checksums | CI | todo | |
| R04 | Migration guide: client config, re-pairing, keeping `messages.db` | live | todo | |
| R05 | README rewritten for the single binary | review | todo | |
| R06 | `SECURITY.md` and the issue page updated | review | todo | |
| R07 | Go and Python code removed | review | todo | Only when everything else is `verified`. |
| R08 | `dev-main` merged into `main`, tagged `v1.0.0` | review | todo | |
