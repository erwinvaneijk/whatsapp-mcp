"""Keep the tests independent of a configuration file on this machine.

whatsapp.py reads ~/.config/whatsapp-mcp/config.yaml when it is imported.
Point WHATSAPP_MCP_CONFIG at an empty file first, so a real file (with a real
token or read_only setting) cannot change what the tests see.
"""

import os
import tempfile

_dir = tempfile.mkdtemp(prefix="whatsapp-mcp-tests-")
_empty = os.path.join(_dir, "config.yaml")
with open(_empty, "w"):
    pass
os.chmod(_empty, 0o600)
os.environ["WHATSAPP_MCP_CONFIG"] = _empty
