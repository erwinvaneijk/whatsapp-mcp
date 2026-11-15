# Security policy

## Supported versions

Security fixes go into the latest release and [`dev-main`](https://github.com/erwinvaneijk/whatsapp-mcp/tree/dev-main). A fix comes out as a new release, with binaries on the [releases page](https://github.com/erwinvaneijk/whatsapp-mcp/releases). Older releases, including 0.9 with the Go bridge, get no fixes: upgrade instead, as [Upgrading to 1.0](docs/upgrading-to-1.0.md) describes.

The original [lharries/whatsapp-mcp](https://github.com/lharries/whatsapp-mcp) is no longer maintained.

## Reporting a vulnerability

Please do not report security problems in a public issue, pull request or discussion.

Report them privately through GitHub instead: open the [Security tab](https://github.com/erwinvaneijk/whatsapp-mcp/security) and click **Report a vulnerability**, or go straight to the [reporting form](https://github.com/erwinvaneijk/whatsapp-mcp/security/advisories/new). Only you and the maintainer can see the report.

Useful to include:

- what an attacker can do, and what they need first (for example: another user on the same computer, someone who can message you, or a web page you visit);
- the steps to reproduce it, and the version you tested (`whatsapp-mcp --version`, or the commit of `dev-main`);
- a suggested fix, if you have one.

## What to expect

This fork is maintained by one person in their spare time, so replies are best effort. I aim to confirm a report within a week and keep you informed while it is fixed. Once a fix is released, the advisory is published, crediting you unless you prefer otherwise.

## Scope

In scope: the `whatsapp-mcp` program, both `bridge` and `serve` (the code in `crates/`), and the release binaries.

Out of scope, please report these to their own projects:

- [whatsapp-rust](https://github.com/oxidezap/whatsapp-rust), the library the bridge uses to talk to WhatsApp;
- WhatsApp itself, through [Meta's bug bounty program](https://bugbounty.meta.com/);
- MCP clients such as Claude Desktop or Cursor.

The bridge and the MCP server are meant to run on your own computer, for one user. The [Security section of the README](README.md#security) describes the protections in place (the bridge token, the media folder, read-only by default, the send allowlist, marking messages from others as data, and keeping message content out of logs). Prompt injection through messages from others is limited, not prevented: a report that shows a way around these protections is welcome.
