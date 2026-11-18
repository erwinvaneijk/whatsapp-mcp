# Claude Code plugin

Make whatsapp-mcp installable as a Claude Code plugin, from a marketplace in this repository:

```sh
claude plugin marketplace add erwinvaneijk/whatsapp-mcp
claude plugin install whatsapp-mcp@erwinvaneijk
```

Starts after [release 1.0](../rust-1.0/plan.md), when one `whatsapp-mcp` binary is both the bridge and the MCP server, and Go is no longer needed.

## Decisions

Made on 2026-11-14:

- **Self-hosted marketplace first.** The marketplace lives in this repository. A listing in Anthropic's plugin directory may follow later; see [Directory listing](#directory-listing-later).
- **After 1.0.** The plugin is built on the 1.0 release binaries, not on 0.9 with the Go bridge.
- **Names** (2026-11-15): the plugin is `whatsapp-mcp`, as the repository and the program; the marketplace is `erwinvaneijk`, with room for more plugins. A directory listing may need a name without "WhatsApp" in it.
- **Read-only by default.** The MCP server is read-only unless [full control](../../README.md#full-control-sending) is on (the default since 1.0). Users turn on sending themselves, knowing the prompt-injection risk.

## What a plugin can and cannot do

Checked against the Claude Code plugin docs on 2026-11-13 ([manifest](https://code.claude.com/docs/en/plugins/manifest-reference.md), [components](https://code.claude.com/docs/en/plugins/components.md), [marketplaces](https://code.claude.com/docs/en/plugins/marketplace-reference.md)).

- **MCP server:** a stdio server in `.mcp.json`, started by Claude Code for each session and stopped when the session ends. `${CLAUDE_PLUGIN_ROOT}` is the installed plugin (it changes on every update), and `${CLAUDE_PLUGIN_DATA}` is a folder that survives updates.
- **Settings:** `userConfig` in `plugin.json` asks the user for values on install, which `.mcp.json` uses as `${user_config.<key>}`.
- **No long-running processes:** MCP servers and monitors only run during a session. The bridge must receive messages all the time, so the plugin does not run it. It helps the user install the bridge as a service instead.
- **No per-platform binaries:** a plugin has no way to pick a binary per OS and architecture, and the directory holds any compiled executable for review. The plugin ships a small launcher script that downloads the release binary instead.

## Design

```
.claude-plugin/marketplace.json     the marketplace: one plugin, source "./plugin"
plugin/
  .claude-plugin/plugin.json        name, version, description, author, license, links, userConfig
  .mcp.json                         the MCP server, started through the launcher
  scripts/launch.sh                 the launcher, for macOS, Linux and Windows (Git Bash)
  scripts/SHA256SUMS                checksums of the release binaries this plugin version runs
  skills/whatsapp-setup/SKILL.md    setup: config file, bridge service, linking
  README.md                         what the plugin downloads, runs and stores
```

**Launcher.** `launch.sh serve` finds the OS and architecture, and checks `${CLAUDE_PLUGIN_DATA}/bin/whatsapp-mcp` against the checksum this plugin pins. One script serves all three platforms: Claude Code runs on Windows through Git Bash, so `.mcp.json` starts it with `bash`, and no PowerShell launcher is needed. When the binary is missing or its checksum differs, the launcher downloads it from the GitHub release, checks it against `SHA256SUMS`, and stops with a clear error if the checksum is wrong. Then it runs the binary with `exec`. The bridge service runs the same binary, so plugin updates also update the bridge on its next restart.

**`.mcp.json`:**

```json
{
  "mcpServers": {
    "whatsapp": {
      "command": "bash",
      "args": ["${CLAUDE_PLUGIN_ROOT}/scripts/launch.sh", "serve"],
      "env": {
        "WHATSAPP_FULL_CONTROL": "${user_config.full_control}",
        "WHATSAPP_MCP_CONFIG": "${user_config.config_file}"
      }
    }
  }
}
```

**`userConfig`:**

| Key | Type | Default | Purpose |
|---|---|---|---|
| `full_control` | boolean | `false` | Offers the send tools. The bridge needs `full_control` in the configuration file too, since the plugin doesn't start it. |
| `config_file` | file | none (empty: `~/.config/whatsapp-mcp/config.yaml`) | The [configuration file](../../README.md#configuration) that both the bridge and the server read. |

The store folder comes from `store_dir` in `config.yaml`, so `serve` needs no `--store`. The bridge token stays in `config.yaml` (mode `0600`), not in `userConfig`: the bridge runs as a service outside Claude Code and cannot read Claude's secure store.

**Setup skill.** `whatsapp-setup` walks the user through, with the program the launcher downloaded:

1. Creating `config.yaml` with a fresh token and `store_dir`, and optionally `media_dir`, `send_allowlist` and `full_control`.
2. Linking with `whatsapp-mcp link` (QR code or pairing code), in a terminal. The skill recommends trying a test account first.
3. Installing the bridge as a user service with `whatsapp-mcp service install` (launchd on macOS, systemd on Linux; since 1.0 the program writes these itself), or a scheduled task on Windows, as the README shows.
4. Checking that `messages.db` fills and that the tools answer.

It also explains how to stop the service and remove the store folder when uninstalling.

## Steps

Each step is one pull request.

| Step | Work | Done when |
|---|---|---|
| 1. Release binaries — **done** (v1.0.0) | Part of 1.0 phase 6. A release workflow builds `whatsapp-mcp` for macOS (arm64, x86_64), Linux (x86_64, arm64) and Windows (x86_64) on a `v*` tag, and attaches the binaries and `SHA256SUMS` to the GitHub release. | A test tag produces all five binaries; each prints the right `--version`; the checksums match. |
| 2. Plugin folder — **done** | `plugin/` as in [Design](#design): manifest, `.mcp.json`, launcher, pinned checksums, README. | `claude plugin validate plugin --strict` passes. The launcher downloads, verifies and starts the binary on macOS, Linux and Windows, and refuses a binary with a wrong checksum (CI: `plugin.yml`). Without full control the send tools are hidden, and turning it on shows them. |
| 3. Setup skill — **built** | `skills/whatsapp-setup/SKILL.md`, using `whatsapp-mcp link` and `service install`. | On a clean user account on macOS and Linux, following the skill gives a linked bridge that survives a logout or reboot, and working tools in Claude Code. |
| 4. Marketplace — **built** | `.claude-plugin/marketplace.json` at the repository root, with a distinctive name that is not one of the reserved marketplace names. `claude plugin validate` on the marketplace too, in CI (the plugin is already checked there). | `claude plugin marketplace add erwinvaneijk/whatsapp-mcp` and `claude plugin install` work on a clean machine. CI fails on an invalid manifest. |
| 5. Evals — **built** (pass locally, 3 of 3 runs per case; CI runs them once the `ANTHROPIC_API_KEY` secret is set) | `claude plugin eval` cases with mocked tools in `evals/mocks/`, so no WhatsApp account is needed: without full control the send tools are hidden; a message with injected instructions is not acted on; `list_messages` with time filters. | The evals pass in CI. |
| 6. Release — **done** ([releasing](../releasing.md); with 1.1.0, `claude plugin update` from GitHub went from 1.0.0 to 1.1.0, and the launcher replaced the 1.0.0 binary) | A release process: bump `version` in `plugin.json` and the pinned `SHA256SUMS` with each binary release, and tag. README: an "Install as a Claude Code plugin" section. | A plugin update with `claude plugin update` moves both the server and (after a restart) the bridge to the new version. |

## Directory listing (later)

Anthropic's plugin directory takes submissions at claude.ai/directory/manage ([checklist](https://claude.com/docs/plugins/pre-submission-checklist)). Before submitting:

- Read the [directory policy](https://support.claude.com/en/articles/13145358-anthropic-software-directory-policy). This is an unofficial WhatsApp client, and it can be used for prompt injection and data leaks (the lethal trifecta); both may stop a listing.
- A launcher that downloads a binary, and any compiled code, is held for a reviewer. Explain the download and the pinned checksums in the submission.
- The README must name everything the plugin runs, sends or fetches. The repository must stay under 50 MiB archived and 10,000 files, without submodules or secrets.

## Risks

- **The bridge service differs per platform.** Three service formats to write and test. Windows has the least testing so far (phase 1 deferred it).
- **First start is slow.** The first session downloads the binary before the MCP server starts. If the download takes too long, Claude Code may report the server as failed; the setup skill downloads it ahead of time to avoid this.
- **Version skew.** The server and the bridge run the same binary but restart at different times. The bridge API is kept stable for 1.0, so a server one version ahead of the bridge must keep working.
