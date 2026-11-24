# History

This server began as [lharries/whatsapp-mcp](https://github.com/lharries/whatsapp-mcp): a Go bridge to WhatsApp, built on whatsmeow, and an MCP server in Python. People liked it, and sent fixes. But upstream has had no commits since July 2025, and those fixes stayed waiting as unmerged pull requests.

This fork started by collecting them. Each upstream pull request came in on a branch of its own, and fixes of this fork's own were added next to them: over 50 changes in all. [Branches in `dev-main`](branches.md) lists each one with what it does, and [Upstream issues and this fork](upstream-issues.md) shows, for every open upstream issue, whether this fork fixes it.

Then it was rewritten. Version 0.9 replaced the Python server with Rust; version 1.0 replaced the Go bridge too, so the whole thing is one program, `whatsapp-mcp`. A [parity harness](../parity/README.md) checked that the tools give the same answers as before, and the Rust bridge ran a week of daily use before 1.0. On the way it became safer by default: nothing is sent unless you turn on [full control](../README.md#full-control-sending). The [1.0 plan](archive/rust-1.0/plan.md) tells the rest.
