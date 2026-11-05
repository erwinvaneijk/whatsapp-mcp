import os
import stat
import sys

import pytest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

import config  # noqa: E402

EXAMPLE = """
# Settings for the bridge and the MCP server
bridge_token: "0123456789abcdef0123456789abcdef"
media_dir: /home/me/whatsapp-outbox
read_only: yes
pair_phone: 0612345678        # kept as written, not read as a number
log_content:                  # empty: not set
send_allowlist:
  - 31612345678               # Maria
  - "+31 6 8765 4321"         # Tom
  - 120363000000000000@g.us   # Book club
"""


def test_parse_config():
    assert config.parse_config(EXAMPLE) == {
        "WHATSAPP_BRIDGE_TOKEN": "0123456789abcdef0123456789abcdef",
        "WHATSAPP_MEDIA_DIR": "/home/me/whatsapp-outbox",
        "WHATSAPP_MCP_READ_ONLY": "yes",
        "WHATSAPP_PAIR_PHONE": "0612345678",
        "WHATSAPP_SEND_ALLOWLIST": "31612345678, +31 6 8765 4321, 120363000000000000@g.us",
    }
    assert config.parse_config("send_allowlist: 316123, 316456") == {
        "WHATSAPP_SEND_ALLOWLIST": "316123, 316456"
    }
    assert config.parse_config("# only comments\n") == {}


@pytest.mark.parametrize(
    "text",
    [
        "bridge_tokn: x",  # typo in a name
        "media_dir: [a, b]",  # list for a single value
        "send_allowlist:\n  - a: b",  # nested entry in a list
        "- just a list",  # not settings
        "bridge_token: [unclosed",  # invalid YAML
    ],
)
def test_parse_config_errors(text):
    with pytest.raises(config.ConfigError):
        config.parse_config(text)


def test_error_names_line_and_setting():
    with pytest.raises(config.ConfigError, match=r"line 2: unknown setting 'send_alowlist'"):
        config.parse_config("read_only: yes\nsend_alowlist: 1")


def write(tmp_path, text, mode=0o600):
    path = tmp_path / "config.yaml"
    path.write_text(text)
    os.chmod(path, mode)
    return path


def test_load_config_environment_wins(tmp_path, monkeypatch):
    path = write(tmp_path, "media_dir: /from/file\nread_only: yes\n")
    monkeypatch.setenv("WHATSAPP_MCP_CONFIG", str(path))
    monkeypatch.setenv("WHATSAPP_MEDIA_DIR", "/from/env")
    monkeypatch.setenv("WHATSAPP_MCP_READ_ONLY", "")
    assert config.load_config() == path
    assert os.environ["WHATSAPP_MEDIA_DIR"] == "/from/env"
    assert os.environ["WHATSAPP_MCP_READ_ONLY"] == "yes"


def test_missing_files(tmp_path, monkeypatch):
    monkeypatch.delenv("WHATSAPP_MCP_CONFIG")
    monkeypatch.setenv("HOME", str(tmp_path))  # no default file: fine
    assert config.load_config() is None
    monkeypatch.setenv("WHATSAPP_MCP_CONFIG", str(tmp_path / "missing.yaml"))
    with pytest.raises(config.ConfigError, match="does not exist"):
        config.load_config()


@pytest.mark.skipif(os.name == "nt", reason="Windows permissions work differently")
def test_refuses_file_others_can_read(tmp_path, monkeypatch):
    monkeypatch.setenv("WHATSAPP_MCP_CONFIG", str(write(tmp_path, "bridge_token: secret\n", 0o644)))
    with pytest.raises(config.ConfigError, match="chmod 600"):
        config.load_config()


def test_read_only_from_file_hides_send_tools(tmp_path, monkeypatch):
    """read_only in the file reaches main.py, which reads it after the file is applied."""
    monkeypatch.setenv("WHATSAPP_MCP_CONFIG", str(write(tmp_path, "read_only: true\n")))
    monkeypatch.setenv("WHATSAPP_MCP_READ_ONLY", "")  # restored after the test
    config.load_config()
    sys.modules.pop("main", None)
    import asyncio
    import main

    try:
        names = {t.name for t in asyncio.run(main.mcp.list_tools())}
        assert "send_message" not in names and "list_messages" in names
    finally:
        sys.modules.pop("main", None)


def test_example_file_is_valid():
    """config.example.yaml must stay valid; its token is empty on purpose."""
    example = os.path.join(
        os.path.dirname(os.path.abspath(__file__)), "..", "..", "config.example.yaml"
    )
    values = config.parse_config(open(example).read())
    assert values.get("WHATSAPP_BRIDGE_TOKEN", "") == ""
    assert set(values) <= set(config.SETTINGS.values())
