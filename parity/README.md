# Parity harness

Records what the MCP server shows to an MCP client, so a new implementation (the Rust rewrite, see [docs/rust-1.0](../docs/rust-1.0/plan.md)) can be checked against it. It is also a regression test for today's server.

For each **profile** in [`cases.json`](cases.json) the harness:

1. builds a temporary workspace with test databases, a media folder and a config file;
2. starts a fake bridge that records the requests it gets and answers as the case says;
3. starts the MCP server over stdio, as an MCP client would;
4. records the server name, instructions and tool list (names, descriptions, input and output schemas, annotations);
5. calls each case's tool and records the result (text and structured content, error flag) and the requests that reached the bridge;
6. compares everything with the golden files in [`golden/`](golden/), one folder per profile, one file per case.

## Running

```bash
cd parity
uv run --locked python run.py              # compare; exit code 1 on any difference
uv run --locked python run.py --profile default
uv run --locked python run.py --update     # record the current output as the new golden files
```

CI runs the comparison on every push. When a change to the server alters its output on purpose, run `--update` and commit the golden files with the change; the diff shows the change for review.

## Test data

All names, numbers and messages are invented. Never put real contact or chat data in here.

| File | Contents |
|---|---|
| [`fixture/schema.sql`](fixture/schema.sql) | The `messages.db` schema exactly as the bridge creates it. Generated, and checked in CI, by `TestParitySchemaMatchesBridge` in `whatsapp-bridge/parity_schema_test.go`. |
| [`fixture/messages.sql`](fixture/messages.sql) | Chats and messages: direct, group and LID chats, one person under two chats, replies, captions, media, reactions, edits, times in summer and winter time and in UTC, text that tries prompt injection. |
| [`fixture/whatsmeow.sql`](fixture/whatsmeow.sql) | The two whatsmeow tables the server reads from `whatsapp.db` for names: contacts and the LID-to-phone map. |

The media folder gets `photo.jpg` and `voice.ogg`; the workspace also has `outside.txt`, outside the media folder.

## Profiles

| Profile | What differs |
|---|---|
| `default` | Token, media folder, full-text search, `TZ=Europe/Amsterdam`. Most cases. |
| `read_only_env`, `read_only_config` | Read-only mode, set in the environment or in the config file: no send tools. |
| `new_york` | `TZ=America/New_York`: times without a zone are local to the server. |
| `no_fts` | Database without the full-text index: search falls back to a scan. |
| `bridge_down` | Nothing listens on the bridge address. |
| `unconfigured` | No bridge token and no media folder. |

A profile can set `env`, `config` (text of the config file), `tz`, `fts`, `token`, `media_dir`, `bridge` (`"down"`) and `normalize` (regular expressions applied to results, for text that differs per run, such as a connection error from the OS).

## Cases

```json
"send_message-refused": {
  "tool": "send_message",
  "args": {"recipient": "31699999999", "message": "not allowed"},
  "bridge": {"status": 403, "json": {"success": false, "message": "..."}}
}
```

- `args` may use `<MEDIA>`, `<STORE>` and `<WORKSPACE>`; they are replaced with the real paths, and real paths in results are replaced back.
- `bridge` is the fake bridge's answer, as `json` or as a plain-text `body`. Without it, the bridge answers as a working bridge would (`DEFAULT_BRIDGE_RESPONSES` in `run.py`). Use the answers the real bridge gives (`whatsapp-bridge/main.go`, `reaction.go`, `allowlist.go`).
- Cases in a profile share one server, so they must not depend on each other.

## Testing another implementation

The harness starts the server with the command for `--server` (see `SERVERS` in `run.py`), or any command given with `--server-cmd`. That command gets the normal settings (`WHATSAPP_BRIDGE_TOKEN`, `WHATSAPP_MEDIA_DIR`, `WHATSAPP_MCP_CONFIG`, ...) plus:

- `PARITY_STORE_DIR`: the folder with `messages.db` and `whatsapp.db`;
- `PARITY_BRIDGE_URL`: the bridge API base, such as `http://127.0.0.1:54321/api`.

[`adapters/python_server.py`](adapters/python_server.py) does this for the Python server without changing it. The Rust server will get a hidden option for the same purpose.

Time zones are set through `TZ`, so the harness runs on macOS and Linux, not on Windows.
