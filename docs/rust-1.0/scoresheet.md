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

Last updated: 2026-11-06 (phase 5, step 1: the Rust store writes `messages.db` as the Go bridge does).

| Area | Rows | todo | wip | done | verified | changed / dropped |
|---|---|---|---|---|---|---|
| MCP interface | 20 | 0 | 0 | 19 | 0 | 1 |
| Configuration | 12 | 0 | 6 | 6 | 0 | 0 |
| Bridge: WhatsApp | 17 | 16 | 1 | 0 | 0 | 0 |
| Bridge: API and security | 7 | 7 | 0 | 0 | 0 | 0 |
| Bridge: storage | 6 | 0 | 1 | 5 | 0 | 0 |
| Build, release, docs | 9 | 5 | 2 | 2 | 0 | 0 |
| **Total** | **71** | **28** | **10** | **32** | **0** | **1** |

## MCP interface

What Claude and other MCP clients see. Today: `whatsapp-mcp-server/main.py` and `whatsapp.py`.

| ID | Feature | Check | Status | Notes |
|---|---|---|---|---|
| M01 | Server name `whatsapp` and the server instructions text | harness | done | Harness: `_server.json` in every profile matches (`run.py --server rust --only _server`). |
| M02 | Tool list: names, titles, descriptions, parameter schemas and defaults | harness | done | Definitions are data (`crates/mcp/src/tools.json`); `crates/mcp/tests/definitions.rs` keeps them equal to the golden file. |
| M03 | Tool annotations (read-only, destructive, idempotent, open-world) | harness | done | Part of the definitions; checked as M02. |
| M04 | Read-only mode hides the send tools | harness | done | `read_only` / `WHATSAPP_MCP_READ_ONLY` Harness profiles `read_only_env` and `read_only_config`; `definitions.rs` checks exactly the four send tools are hidden. |
| M05 | `search_contacts` | harness | done | Names and LID-to-phone map; see plan, "Contact names". Harness: Rust matches the golden files (phase 4, step 2). |
| M06 | `list_messages`: filters (time, sender, chat), paging, sorting | harness | done | Harness: Rust matches the golden files (phase 4, step 2). Time filters also in Amsterdam and New York, and across the DST change. |
| M07 | `list_messages`: text search, FTS5 and the fallback scan | harness | done | Rust always has FTS5; the fallback is still needed for a database without the index. Harness: Rust matches the golden files (phase 4, step 2). Both with the FTS5 index (`default`) and without (`no_fts`). |
| M08 | `list_messages`: context around each match | harness | done | Harness: Rust matches the golden files (phase 4, step 2). |
| M09 | `list_chats`: search, paging, sort, with and without last message | harness | done | Harness: Rust matches the golden files (phase 4, step 2). |
| M10 | `get_chat`, `get_direct_chat_by_contact`, `get_contact_chats` | harness | done | Harness: Rust matches the golden files (phase 4, step 2). |
| M11 | `get_last_interaction`, `get_message_context` | harness | done | Harness: Rust matches the golden files (phase 4, step 2). |
| M12 | `send_message`: text, reply, mentions | harness | done | Against a fake bridge that records requests. Harness: Rust matches the golden files (phase 4, step 3). |
| M13 | `send_file`: caption, only from `media_dir` | harness | done | Harness: Rust matches the golden files (phase 4, step 3). Media folder checks also unit-tested (`..`, symlinks, shared prefix). |
| M14 | `send_audio_message`: ffmpeg conversion, temp file cleanup | harness, tests | done | Harness: Rust matches the golden files (phase 4, step 3). Only `.ogg` in the harness; the ffmpeg conversion and temp file cleanup are unit-tested (CI installs ffmpeg on Linux x86_64). |
| M15 | `download_media`: returns the file path | harness | done | Harness: Rust matches the golden files (phase 4, step 3). |
| M16 | `react_to_message`, including removing a reaction | harness | done | Harness: Rust matches the golden files (phase 4, step 3). |
| M17 | Structured output: same JSON fields and types as today | harness | done | The Python MCP SDK (`mcp` 2.x) validates return types against the declared output schema; the Rust server must declare and return the same schemas. All tools: text and structured content match (step 3). |
| M18 | Messages from others marked as data (`<message_content>`, tag neutralising, names on one line, read-tool notice) | harness, tests | done | Harness: Rust matches the golden files (phase 4, step 2). Unit tests in `crates/mcp/src/untrusted.rs`. |
| M19 | Times shown with UTC offset; `after`/`before` accept ISO-8601, naive times are local | harness, tests | done | Harness: Rust matches the golden files (phase 4, step 2). Unit tests in `crates/store/src/time.rs`. |
| M20 | Error texts for bad input and an unreachable bridge | harness | changed | Decided: today's quirks get better behaviour in Rust, so this row becomes `changed` when done; see [interface.md, section 5](interface.md#5-behaviour-that-improves-in-10). Eleven cases have Rust-specific golden files (`*.rust.json`): bad input and bridge errors give their reason. Bridge-down and missing-token messages are unchanged. |

## Configuration

Same file, same environment variables, same rules. Today: `whatsapp-bridge/config.go` and `whatsapp-mcp-server/config.py`.

| ID | Feature | Check | Status | Notes |
|---|---|---|---|---|
| C01 | File at `~/.config/whatsapp-mcp/config.yaml`, or the path in `WHATSAPP_MCP_CONFIG` (must exist when named) | tests | done | `whatsapp-mcp-config`: `Config::load`; tests in `crates/config/tests/load.rs`. `~/` at the start of `WHATSAPP_MCP_CONFIG` means the home folder, as in Python. |
| C02 | A set environment variable wins; an empty one counts as unset | tests | done | `load.rs`: `environment_wins_and_empty_counts_as_unset`. |
| C03 | Values keep their exact text (`0612` stays `0612`) | tests | done | Works on the YAML parser's events, so values keep their text; shared cases in `parity/config-cases.json`. |
| C04 | `send_allowlist` as a YAML list or a separated string | tests | done | Shared cases: block list, flow list, empty list, one line. |
| C05 | Unknown setting or wrong type is an error with the line number | tests | done | Shared cases check the line number and the name. Stricter than today: a setting given twice, several YAML documents, and YAML aliases are errors too. |
| C06 | Refuse a file others can read or change (not on Windows) | tests | done | `load.rs`: modes 644, 620 and 604 are refused; not checked on Windows. |
| C07 | `bridge_token` / `WHATSAPP_BRIDGE_TOKEN`: bridge refuses to start without one (32+ characters) | tests | wip | `Config::bridge_token` refuses fewer than 32 characters. The bridge uses it from phase 5. |
| C08 | `media_dir` / `WHATSAPP_MEDIA_DIR` | tests | wip | `Config::media_dir`. Used from phase 4. |
| C09 | `send_allowlist` / `WHATSAPP_SEND_ALLOWLIST`: a bad entry stops the bridge | tests | wip | `SendAllowlist::parse`, with the bridge's rules; shared cases run against Go and Rust. Matching a LID to its phone number stays in the bridge (phase 5). |
| C10 | `read_only` / `WHATSAPP_MCP_READ_ONLY` | tests | wip | `Config::read_only`. Used by `serve` from phase 4. |
| C11 | `log_content` / `WHATSAPP_LOG_CONTENT` | tests | wip | `Config::log_content`. Used by the bridge from phase 5. |
| C12 | `pair_phone` / `WHATSAPP_PAIR_PHONE` | tests | wip | `Config::pair_phone`. Used by the bridge from phase 5. |

## Bridge: WhatsApp

Today: `whatsapp-bridge/main.go` and friends, on whatsmeow. Library rows are tested first in the phase 1 go/no-go test; its results are in [phase1.md](phase1.md).

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
| W10 | Contact names, push names, LID-to-phone map | tests, live | wip | Copied into `messages.db`; see plan. Phase 5, step 1: tables `contacts` and `lid_map` in `messages.db`, read by `serve` when they exist. The bridge fills them from step 3 on. |
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
| S01 | Same `chats` and `messages` schema; opens an existing database without loss | tests | wip | Tested on a copy of a real database. Phase 5, step 1: the Rust schema is the Go bridge's, byte for byte (`crates/store/tests/write.rs` compares it with `parity/fixture/schema.sql`). The test on a copy of a real database is still to do. |
| S02 | Same migrations (e.g. `quoted_message_id`), safe to run repeatedly | tests | done | Phase 5, step 1: `quoted_message_id` added to older databases; opening twice changes nothing (tests). |
| S03 | FTS5 table and triggers | tests | done | Phase 5, step 1: always created (bundled FTS5), rebuilt on open; leftover triggers do not break inserts (test ported from `fts_test.go`). |
| S04 | Indexes, including the `julianday()` time indexes | tests | done | Phase 5, step 1: old time indexes replaced by `julianday()` ones (test ported from `moment_index_test.go`). |
| S05 | WAL mode | tests | done | Phase 5, step 1: also foreign keys, normal sync and a 5 s busy wait, as Go (test). |
| S06 | Last message per chat chosen by row, not timestamp | tests | done | The read queries join the latest message by rowid (phase 4); the parity harness checks it. |

## Build, release, docs

| ID | Feature | Check | Status | Notes |
|---|---|---|---|---|
| R01 | Parity harness and fixture database, passing against today's code | CI | done | Phase 0: [`parity/`](../../parity/README.md), 93 golden files, `Parity harness` CI job. |
| R02 | CI: rustfmt, clippy, tests on macOS, Linux, Windows | CI | done | Phase 2: `Rust (…)` CI jobs on Linux x86_64 and arm64, macOS arm64 and x86_64, Windows x86_64 (macOS arm64 in a workflow of its own, `macos-arm64.yml`, because GitHub often has no runner for it); see [crates/README.md](../../crates/README.md). |
| R03 | Release binaries: macOS arm64 and x86_64, Linux x86_64 and arm64, Windows x86_64, with checksums | CI | todo | |
| R04 | Migration guide: client config, re-pairing, keeping `messages.db` | live | todo | |
| R05 | README rewritten for the single binary | review | wip | 0.9: installation, client configuration and an upgrade section rewritten for the Rust MCP server. The full rewrite comes with the Rust bridge (1.0). |
| R06 | `SECURITY.md` and the issue page updated | review | todo | |
| R07 | Go and Python code removed | review | wip | 0.9: the Python server is removed. The Go bridge goes in 1.0, when everything else is `verified`. |
| R08 | `dev-main` merged into `main`, tagged `v1.0.0` | review | todo | |
| R09 | Dependency checks (vulnerabilities, licenses, sources) and no `unsafe` code | CI | done | Phase 2: `cargo deny check` in the `Rust dependencies` CI job ([deny.toml](../../deny.toml)); `unsafe_code = "forbid"` for every crate. |
