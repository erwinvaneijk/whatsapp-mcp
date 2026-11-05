# 1.0: rewrite in Rust

Status: draft, for discussion. Progress is tracked in the [score sheet](scoresheet.md).

## Goal

Replace the Go bridge and the Python MCP server with one Rust program, as a drop-in replacement:

- **Same MCP interface.** The same tools, with the same names, parameters, defaults, descriptions, annotations and output, and the same server instructions. An MCP client such as Claude Desktop cannot tell the difference, except for the command that starts the server.
- **Same configuration.** The same `config.yaml` (location, `WHATSAPP_MCP_CONFIG`, settings, rules) and the same environment variables.
- **Same data.** The existing `store/messages.db` is used as is, so message history is kept.

## What Rust does and does not improve

To set expectations:

- **Fewer moving parts:** one binary instead of Go + Python + uv + a virtual environment. The install is simpler, and there is less to keep patched.
- **Smaller supply chain:** dependencies are pinned in `Cargo.lock` and checked in CI (`cargo audit`, `cargo deny`). Our own code forbids `unsafe`.
- **One code base:** one config parser, one set of types for the tools, one test suite. Today the config rules exist twice, in Go and in Python, and must be kept in sync by hand.
- **Not memory safety:** Go and Python are already memory-safe, so that is not where the gain is.
- **Main risk:** the WhatsApp protocol library. whatsmeow (Go) is mature and widely used. The Rust option, [whatsapp-rust](https://github.com/jlucaso1/whatsapp-rust), is younger:
  - Facts as of 2026-11-05: MIT license, about 1,000 stars, about 30 contributors, active daily, crate version 0.7.
  - Its README calls macOS and Windows "unqualified".
  - It implements the Signal protocol itself.

  That is why phase 1 below is a go/no-go test.

## Architecture

One binary, `whatsapp-mcp`, with two commands:

| Command | Replaces | What it does |
|---|---|---|
| `whatsapp-mcp bridge` | `go run main.go` | Long-running: keeps the WhatsApp connection, stores messages in `store/messages.db`, serves the local API on `127.0.0.1:8080` with the bridge token. |
| `whatsapp-mcp serve` | `uv run main.py` | Started by the MCP client over stdio: reads `messages.db`, and sends through the bridge's API. |

The split stays because the MCP server only runs while a client is open, but messages must be received all the time.

**Contact names:** today the Python server reads names and the LID-to-phone map straight from whatsmeow's own tables in `whatsapp.db`. The Rust bridge will instead copy them into two tables of its own in `messages.db` (`contacts`, `lid_map`). `serve` then never depends on a library's private storage format.

Cargo workspace:

| Crate | Contents |
|---|---|
| `config` | `config.yaml` and environment variables, with the same rules as today. |
| `store` | `messages.db`: schema, migrations, FTS5, queries. Uses rusqlite with bundled SQLite, so FTS5 is always available and the `-tags sqlite_fts5` build option disappears. |
| `bridge` | WhatsApp connection (whatsapp-rust), event handling, sending, media, retry, allowlist, local API. |
| `mcp` | The tools, on [rmcp](https://github.com/modelcontextprotocol/rust-sdk), the official Rust MCP SDK (crate version 3.8 as of 2026-11-05; pinned in `Cargo.lock`). |
| `whatsapp-mcp` | The binary and its commands. |

## How we know it is a drop-in

A **parity harness**, written first and against today's code:

1. **Fixture database:** a fixture `messages.db` with invented chats and messages, never real data. It covers groups, LID chats, replies, captions, reactions, time zones and text that tries prompt injection.
2. **Recording:** a script starts the MCP server over stdio and calls every tool with a fixed set of arguments. It records the tool list (names, schemas, annotations, descriptions), the server instructions and every result as golden files. Send tools run against a fake bridge that records the requests.
3. **Comparison:** the same script runs against the Rust server and must produce identical output.
4. **Config:** the config tests (valid files, every error case, precedence, permissions) run against both programs.

The harness is the definition of "the same interface". A difference is either fixed, or written down in the score sheet as an intended change.

## Phases

Each phase ends in something that works, and nothing is removed until the replacement is verified.

| Phase | Work | Done when |
|---|---|---|
| 0. Freeze the interface | Write the parity harness and fixture against the current code. Write down the interface: tools, config, the bridge API, the `messages.db` schema. | Harness passes against today's code in CI. |
| 1. Library test (go/no-go) | Small Rust program on whatsapp-rust, on macOS, Linux and Windows, with a separate WhatsApp test account (a spare number is available): QR and code pairing, receiving, history sync, sending text, media and reactions, replies and mentions, LID to phone mapping, contact names, expired-media retry. | Every item works, or has a workaround. **If not:** keep the Go bridge for 1.0 and only replace the Python server (phases 2–4). |
| 2. Workspace and CI | Cargo workspace, `rustfmt`, `clippy -D warnings`, tests, `cargo audit`/`deny`, `#![forbid(unsafe_code)]`, builds for macOS (arm64, x86_64), Linux and Windows. | CI green on all targets. |
| 3. Config | `config` crate, with the Go and Python config tests ported. | Config parity tests pass. |
| 4. Rust MCP server | `serve`: all read tools on `messages.db`; send tools through the existing Go bridge's API. Contact names read from whatsmeow's `whatsapp.db`, as the Python server does today. | Parity harness passes. The Python server can be switched off. **Release 0.9**: Rust server + Go bridge. |
| 5. Rust bridge | `bridge`: connection, pairing, storing (same schema and migrations), history sync, outgoing messages, media, retry, allowlist, API, file permissions, log privacy. | Bridge tests ported and passing; runs for a week alongside daily use without problems. |
| 6. Migration and packaging | Migration guide (Claude Desktop config, re-pairing if needed), release binaries with checksums, README rewrite. | Fresh install and upgrade both tested from the guide. |
| 7. Release 1.0 | Remove Go and Python code, merge `dev-main` into `main`, tag `v1.0.0`. | All score sheet rows "verified". |

Phases 0 and 1 are independent and can run at the same time. Phase 0 is useful even if the rewrite stops: it protects today's code against regressions.

## Things that will change for users

Unavoidable, and covered by the migration guide:

- **Client config:** the MCP client config changes from `uv --directory … run main.py` to the path of the `whatsapp-mcp` binary with `serve`.
- **Re-pairing:** likely. whatsapp-rust keeps the session in its own format, so the WhatsApp link stored in `store/whatsapp.db` probably cannot be carried over. The bridge must then be linked again once, with QR or pairing code. `messages.db`, and with it all history, is kept.
- **Build requirements:** Go, Python, uv and a C compiler (for CGO) are no longer needed to build. ffmpeg is still needed for voice messages.

## Decisions

Made on 2026-11-05:

- **Re-linking is acceptable.** If the whatsmeow session cannot be carried over, users link the bridge again once after upgrading. `messages.db` is kept.
- **Release 0.9 first.** 0.9 is the Rust MCP server with the existing Go bridge, and Python removed. 1.0 adds the Rust bridge.
- **Platforms:** release binaries for macOS (arm64, x86_64), Linux (x86_64, arm64) and Windows (x86_64).
- **Bridge API kept as is for 1.0:** HTTP on `127.0.0.1:8080` with `X-Bridge-Token`. A Unix socket may follow after 1.0.

## Risks

| Risk | Effect | Mitigation |
|---|---|---|
| whatsapp-rust lacks a feature or breaks on a WhatsApp change | Bridge cannot match today's features, or stops working | Phase 1 gate (on all three platforms); fall back to the Go bridge; contribute fixes upstream. |
| whatsapp-rust is less proven than whatsmeow | Protocol or crypto bugs, account flagged | Test account during phases 1 and 5; weeks of parallel use before 1.0. |
| Output differs in small ways (whitespace, field order, time format) | Clients or prompts that relied on it behave differently | Parity harness compares byte for byte. |
| `messages.db` written by two programs during the switch | Corrupt or duplicate rows | Only one bridge runs at a time; the Rust bridge uses the same migrations and is tested on a copy of a real database first. |
| MCP SDK changes (the rmcp crate is on major version 3, and has had breaking releases) | Rework | Pin the version; the harness catches changes in output. |
