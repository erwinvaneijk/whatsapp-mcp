# Changelog

## Unreleased

- **`whatsapp-mcp bridge` (preview, test accounts only):** the Rust bridge links (QR code or `WHATSAPP_PAIR_PHONE`), keeps its session in `store/session.db`, and stores incoming messages, history sync, chat and group names, contacts and the LID map in `messages.db`. It sends text, replies, mentions, files and voice notes, reacts, and downloads media (asking the sender's phone again for expired media), with the same local API as the Go bridge. Keep using the Go bridge for your own account until 1.0. See [phase 5](docs/rust-1.0/phase5.md#step-4-live-test).

## 0.9.0

The MCP server is now a Rust program, `whatsapp-mcp serve`, instead of the Python server. The Go bridge stays as it is until 1.0. See the [1.0 plan](docs/rust-1.0/plan.md).

### What you need to do

Build the server with `cargo build --release`, and point your MCP client at it: see [Upgrading from the Python MCP server](README.md#upgrading-from-the-python-mcp-server). Your configuration file, environment variables, `messages.db` and the bridge's link with your phone stay as they are. Python and uv are no longer needed; building needs Rust (`rustup`) and a C compiler.

### What stays the same

The tools, their names, parameters, descriptions and output, the server instructions, the configuration settings and their rules. The [parity harness](parity/README.md) checks all of it: 93 recorded cases in seven setups, from time zones and full-text search to prompt-injection text.

### What changes

- **Clearer errors** ([details](docs/rust-1.0/interface.md#5-behaviour-that-improves-in-10)):
  - Bad input says what is wrong, such as `'limit' must be 0 or greater, got -1`, instead of `Error executing tool list_messages`.
  - A refused or failed send gives the bridge's reason, such as `This chat is not in WHATSAPP_SEND_ALLOWLIST, …`, instead of the bridge's raw answer.
  - A failed download includes the reason.
- **Stricter configuration file:** a setting given twice, several YAML documents and YAML aliases are errors, with the line number.
- **New options:** `serve --store DIR` names the bridge's store folder, and `--bridge-url URL` the bridge's API.
- **Startup warning:** the server warns at startup when there is no `messages.db` in the store folder.
