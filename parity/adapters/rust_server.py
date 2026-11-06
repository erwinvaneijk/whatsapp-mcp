"""Start the Rust MCP server (whatsapp-mcp serve) for the parity harness.

Points it at the harness's test data and fake bridge with its --store and
--bridge-url options. Builds it first if needed; cargo's own output goes to
stderr, so stdout stays free for MCP.
"""

import os

REPO = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))

args = [
    "cargo",
    "run",
    "--quiet",
    "--locked",
    "--manifest-path",
    os.path.join(REPO, "Cargo.toml"),
    "--package",
    "whatsapp-mcp",
    "--",
    "serve",
    "--store",
    os.environ["PARITY_STORE_DIR"],
    "--bridge-url",
    os.environ["PARITY_BRIDGE_URL"],
]
os.execvp(args[0], args)
