"""Start the Python MCP server (whatsapp-mcp-server) for the parity harness.

The server normally finds its databases next to the bridge and the bridge at
127.0.0.1:8080. The harness points it at test data and a fake bridge instead,
through PARITY_STORE_DIR and PARITY_BRIDGE_URL, without changing the server.
Run from whatsapp-mcp-server's environment, for example:

    uv run --locked --directory whatsapp-mcp-server python ../parity/adapters/python_server.py
"""

import os
import sys

SERVER_DIR = os.path.join(
    os.path.dirname(os.path.abspath(__file__)), "..", "..", "whatsapp-mcp-server"
)
sys.path.insert(0, os.path.abspath(SERVER_DIR))

import whatsapp  # noqa: E402  (needs the path above)

store = os.environ["PARITY_STORE_DIR"]
whatsapp.MESSAGES_DB_PATH = os.path.join(store, "messages.db")
whatsapp.WHATSAPP_DB_PATH = os.path.join(store, "whatsapp.db")
whatsapp.WHATSAPP_API_BASE_URL = os.environ["PARITY_BRIDGE_URL"]

import main  # noqa: E402  (after the paths are set)

main.mcp.run(transport="stdio")
