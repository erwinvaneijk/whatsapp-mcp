# Upgrading to 1.0

In 1.0 the Go bridge in `whatsapp-bridge/` is replaced by `whatsapp-mcp bridge`, part of the same program as the MCP server. This guide moves an existing setup over. It takes a few minutes, and you link WhatsApp again once.

Coming from the Python MCP server (before 0.9)? Do [Upgrading from the Python MCP server](../README.md#upgrading-from-the-python-mcp-server) first, then this guide.

## What stays

- **Your message history.** `messages.db` is kept and used as it is. The Rust bridge applies the same schema and migrations as the Go bridge.
- **Your configuration.** The same configuration file (`~/.config/whatsapp-mcp/config.yaml`) and the same environment variables, with the same rules. The bridge token, the media folder, the send allowlist and read-only mode work as before.
- **Your MCP client's configuration**, if you already run `whatsapp-mcp serve` (0.9). The local API between `serve` and the bridge is the same.

## What changes

- **You link WhatsApp again, once.** The Rust bridge keeps its session in its own format, in `store/session.db`. The Go bridge's link (`store/whatsapp.db`) cannot be carried over. After linking, WhatsApp sends the recent history again. Messages already in `messages.db` are replaced by the same message, not added twice.
- **Build requirements.** Go is no longer needed. Building needs Rust (`rustup`) and a C compiler; or download a release binary. FFmpeg is still needed to convert audio to voice messages.
- **No `sqlite_fts5` tag.** Full-text search is always built in.
- **Logs go to stderr** by default, with a time, a level and a source on every line. Set `log_output` to send them to a file or syslog; see [Logs](../README.md#logs).
- **Contact names** come from `messages.db`, which the Rust bridge fills. `serve` still reads the Go bridge's `whatsapp.db` for names when it is there.
- **Time of sent messages:** the local time of sending, instead of WhatsApp's server time. The two differ by the time a send takes.

## Steps

1. **Stop the Go bridge.** Both bridges use port 8080 and the same `messages.db`; only one may run at a time. Stop it in its terminal (Ctrl-C), or stop the service you run it as.

2. **Back up the store folder.** Copy `whatsapp-bridge/store` (or the folder you use) somewhere safe. It holds `messages.db` with your history.

   ```bash
   cp -Rp whatsapp-bridge/store ~/whatsapp-store-backup
   ```

3. **Get the 1.0 program.** Either download it for your platform from the [releases page](https://github.com/erwinvaneijk/whatsapp-mcp/releases) and check it against `SHA256SUMS`:

   ```bash
   shasum -a 256 -c SHA256SUMS --ignore-missing
   chmod +x whatsapp-mcp-*
   ```

   On Linux, `sha256sum -c SHA256SUMS --ignore-missing` does the same.

   Or pull the new version and build it with `cargo build --release`, which makes `target/release/whatsapp-mcp`.

   If your MCP client points at a binary path, put the new program at that path, or update the path in the client's configuration.

4. **Start the bridge** with the same store folder:

   ```bash
   whatsapp-mcp bridge --store /path/to/whatsapp-bridge/store
   ```

   On the first start it shows a QR code. On your phone open WhatsApp > Linked devices > Link a device, and scan it. To link with a code instead, set `pair_phone` (or `WHATSAPP_PAIR_PHONE`) to your number with country code; the bridge then prints an 8-character code to enter under Link with phone number instead.

   The log shows `History sync complete` when the history has arrived. Keep the bridge running, as you did the Go bridge.

5. **Remove the old link.** Your phone now lists two linked devices. Log out the older one, which was the Go bridge, under Linked devices.

6. **Restart your MCP client** and try a tool, such as `list_chats`.

## Going back

The Rust bridge does not touch `whatsapp.db`, so the Go bridge can still run on the same store folder. Stop `whatsapp-mcp bridge`, check out `v0.9.0` and start the Go bridge as before. If you removed its link in step 5, it asks you to link again. Messages received by the Rust bridge stay in `messages.db`.

If something is wrong with `messages.db`, stop the bridge and put back the backup from step 2.
