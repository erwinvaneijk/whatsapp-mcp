# Rust workspace

The Rust rewrite of the bridge and the MCP server, as described in the [1.0 plan](../docs/rust-1.0/plan.md). Since version 0.9, `whatsapp-mcp serve` is the MCP server (it replaced the Python one). The bridge is still the Go program in [`whatsapp-bridge/`](../whatsapp-bridge/) until 1.0.

## Crates

| Folder | Crate | Contents | Filled in |
|---|---|---|---|
| `config/` | `whatsapp-mcp-config` | `config.yaml` and the environment variables, with the same rules as today | done (phase 3) |
| `store/` | `whatsapp-mcp-store` | `messages.db`: the read queries, contact names, times; schema and migrations follow in phase 5 | phase 4: queries done |
| `mcp/` | `whatsapp-mcp-server` | The MCP tools, over stdio | phase 4: done; the parity harness matches |
| `bridge/` | `whatsapp-mcp-bridge` | The WhatsApp connection, storing messages, the local API | phase 5 |
| `whatsapp-mcp/` | `whatsapp-mcp` | The binary: `whatsapp-mcp bridge` and `whatsapp-mcp serve [--store DIR] [--bridge-url URL]` | each phase |

## Building

You need [rustup](https://rustup.rs), which installs the Rust version pinned in [`rust-toolchain.toml`](../rust-toolchain.toml) by itself, and a C compiler: SQLite is built in, with full-text search. On macOS that is the Xcode command line tools, on Linux `gcc` or `clang`, on Windows the Visual Studio build tools (which Rust needs anyway). No Go or Python.

```bash
cargo build --release          # target/release/whatsapp-mcp
cargo test --workspace
```

## Checks

CI runs these on Linux (x86_64, arm64), macOS (arm64, x86_64) and Windows (x86_64). Run them before you push:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check               # cargo install cargo-deny first
```

- **`cargo deny check`** checks the dependencies for known vulnerabilities (the RustSec database), licenses, duplicates and where they come from. The rules are in [`deny.toml`](../deny.toml).
- **No `unsafe`:** every crate forbids `unsafe` code (`[workspace.lints]` in [`Cargo.toml`](../Cargo.toml)).
- **No stray output on stdout:** clippy warns about `println!`, because `serve` speaks MCP over stdout and any other output there breaks the protocol.
- **The phase 1 test program** in [`spike/whatsapp-rust/`](../spike/whatsapp-rust/) is not part of the workspace. It has its own lock file.
