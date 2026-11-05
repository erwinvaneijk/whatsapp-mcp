# Security policy

## Supported versions

Only the [`dev-main`](https://github.com/erwinvaneijk/whatsapp-mcp/tree/dev-main) branch of this fork gets security fixes. The `main` branch mirrors the original [lharries/whatsapp-mcp](https://github.com/lharries/whatsapp-mcp), which is no longer maintained.

## Reporting a vulnerability

Please do not report security problems in a public issue, pull request or discussion.

Report them privately through GitHub instead: open the [Security tab](https://github.com/erwinvaneijk/whatsapp-mcp/security) and click **Report a vulnerability**, or go straight to the [reporting form](https://github.com/erwinvaneijk/whatsapp-mcp/security/advisories/new). Only you and the maintainer can see the report.

Useful to include:

- what an attacker can do, and what they need first (for example: another user on the same computer, someone who can message you, or a web page you visit);
- the steps to reproduce it, and the commit of `dev-main` you tested;
- a suggested fix, if you have one.

## What to expect

This fork is maintained by one person in their spare time, so replies are best effort. I aim to confirm a report within a week and keep you informed while it is fixed. Once a fix is on `dev-main`, the advisory is published, crediting you unless you prefer otherwise.

## Scope

In scope: the WhatsApp bridge (`whatsapp-bridge/`) and the MCP server (`whatsapp-mcp-server/`) in this repository.

Out of scope, please report these to their own projects:

- [whatsmeow](https://github.com/tulir/whatsmeow), the library the bridge uses to talk to WhatsApp;
- WhatsApp itself, through [Meta's bug bounty program](https://bugbounty.meta.com/);
- MCP clients such as Claude Desktop or Cursor.

The bridge and the MCP server are meant to run on your own computer, for one user. The [Security section of the README](README.md#security) describes the protections in place (the bridge token, the media folder, read-only mode, the send allowlist, marking messages from others as data, and keeping message content out of logs). Prompt injection through messages from others is limited, not prevented: a report that shows a way around these protections is welcome.
