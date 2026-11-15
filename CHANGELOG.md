# Changelog

## Unreleased

- **`store_dir` setting** ([#50](https://github.com/erwinvaneijk/whatsapp-mcp/issues/50)): the store folder can be set in the configuration file (or `WHATSAPP_STORE_DIR`), absolute or starting with `~/`. `bridge`, `serve`, `link` and `service install` use it, so `--store` can be left out, also from the MCP client's configuration; `--store` still overrides it. The startup log says where the store folder comes from.
- **README:** "About this fork" is now [This server's origin story](README.md#this-servers-origin-story), and the example screenshot shows the Rust server (names changed).
- **`whatsapp-mcp link` and `whatsapp-mcp service`** ([#45](https://github.com/erwinvaneijk/whatsapp-mcp/issues/45)): `link` (or `login`) links WhatsApp in a terminal, stores the history sync and stops. `service install` runs the bridge as a user service that starts at login (launchd on macOS, systemd on Linux), for a linked store; `service status` and `service uninstall` go with it. On Windows the README explains a scheduled task. `bridge` without a link and without a terminal now stops and says to run `link` first, instead of logging a QR code nobody sees.
- **Read-only by default** ([#44](https://github.com/erwinvaneijk/whatsapp-mcp/issues/44)). **Changes behaviour:** nothing is sent unless `full_control` (or `WHATSAPP_FULL_CONTROL`) is on. Without it, `serve` leaves out the send tools and the bridge refuses to send or react (HTTP 403); reading, searching and downloads keep working. The old `read_only` still works until 2.0, with a warning: `read_only: no` turns full control on. See [Full control (sending)](README.md#full-control-sending).
- **Code comments after the Go removal:** tests and code no longer point to Go files, tests or functions that are gone. Each test says what it checks instead.
- **Windows is beta:** the Windows binary is built and tested in CI, but has not been tried with WhatsApp yet. README: the newsletter link left over from upstream is gone, and the send allowlist section is now [Control whom the bridge is allowed to send to](README.md#control-whom-the-bridge-is-allowed-to-send-to).
- **Security policy for 1.0:** [SECURITY.md](SECURITY.md) covers the latest release and `dev-main`, the `whatsapp-mcp` program and its release binaries. New issue templates ask for the version and platform, and point security reports to private reporting.
- **The Go bridge is removed.** `whatsapp-mcp bridge` replaces it; see [Upgrading to 1.0](docs/upgrading-to-1.0.md). Your store folder, `whatsapp-bridge/store` by default, stays where it is. To go back to the Go bridge, use `v0.9.0`.
- **README for 1.0:** installation now uses one program, `whatsapp-mcp`, downloaded from a release or built with `cargo`, with `bridge` and `serve`; Go is no longer mentioned or needed. `main` becomes the branch with the latest release.
- **Upgrade guide:** [Upgrading to 1.0](docs/upgrading-to-1.0.md) moves a setup from the Go bridge to `whatsapp-mcp bridge`: what stays, what changes, linking again, and how to go back.
- **Security: SQLite 3.54.1** in `whatsapp-mcp` and in the Go bridge, for [CVE-2026-119375](https://nvd.nist.gov/vuln/detail/CVE-2026-119375): older SQLite could return leftover memory when it opened a database together with a crafted `-wal` file. The Go bridge also builds with Go 1.27.4 (downloaded by `go` when needed), for [GO-2026-6859](https://pkg.go.dev/vuln/GO-2026-6859) in `net/http`. Rebuild both programs; nothing else changes.
- **syslog on macOS:** `log_output: syslog` now writes to macOS's unified log (subsystem `whatsapp-mcp`; see [Logs](README.md#logs)). Before, macOS accepted the lines and dropped them.
- **`whatsapp-mcp bridge` (preview, test accounts only):** the Rust bridge links (QR code or `WHATSAPP_PAIR_PHONE`), keeps its session in `store/session.db`, and stores incoming messages, history sync, chat and group names, contacts and the LID map in `messages.db`. It sends text, replies, mentions, files and voice notes, reacts, and downloads media (asking the sender's phone again for expired media), with the same local API as the Go bridge. Keep using the Go bridge for your own account until 1.0. See [phase 5](docs/rust-1.0/phase5.md#step-4-live-test).
- **Startup log:** `whatsapp-mcp serve` and `whatsapp-mcp bridge` start by logging the version, the configuration file, each setting and where its value comes from (environment, file or default), and the store folder. `serve` also logs where contact names come from and the bridge address; `bridge` logs its session file and whether it is linked. The token and the pairing number are only shown as set or not set. See [phase 5, step 5](docs/rust-1.0/phase5.md#step-5-startup-log).
- **Logging:** a stored message is logged as `← message stored` (or `→` for sent ones), without a second time in brackets; the line already starts with the time.
- **Logging:** every line from `whatsapp-mcp serve` and `whatsapp-mcp bridge` has a time, a level and its source. New settings `log_output` (`stderr`, `stdout`, `syslog` or a file), `log_format` (`text` or `json`) and `log_level`; see [Logs](README.md#logs). **Changes behaviour:** the Rust bridge now logs to stderr instead of stdout by default; set `log_output: stdout` to keep the old place. The QR and pairing codes always go to the terminal. The Go bridge accepts the new settings in the shared file and ignores them.

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
