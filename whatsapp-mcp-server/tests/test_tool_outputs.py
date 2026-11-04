"""Call every MCP tool through the server, as a client would.

mcp 2.x checks each tool's return value against its declared return type, so
a tool that returns something other than what it declares fails every call
("Error executing tool ..."). Calling the functions directly does not catch
that; this test goes through MCPServer.call_tool instead.
"""

import asyncio
import os
import sqlite3
import sys
from unittest.mock import Mock

import pytest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

import whatsapp  # noqa: E402

CHAT = "123@g.us"
DM = "31612345678@s.whatsapp.net"

CALLS = {
    "search_contacts": {"query": "Sien"},
    "list_messages": {"limit": 5},
    "list_chats": {"limit": 5},
    "get_chat": {"chat_jid": CHAT},
    "get_direct_chat_by_contact": {"sender_phone_number": "31612345678"},
    "get_contact_chats": {"jid": DM},
    "get_last_interaction": {"jid": DM},
    "get_message_context": {"message_id": "M2", "before": 1, "after": 1},
    "download_media": {"message_id": "M1", "chat_jid": CHAT},
    "send_message": {"recipient": CHAT, "message": "hi @31612345678", "mentions": ["31612345678"]},
    "send_file": {"recipient": CHAT, "media_path": "PHOTO", "caption": "look"},
    "send_audio_message": {"recipient": CHAT, "media_path": "VOICE"},
    "react_to_message": {"message_id": "M1", "chat_jid": CHAT, "emoji": "👍"},
}

# Lookups that find nothing return None; that must not be an error either.
EMPTY_CALLS = {
    "get_chat": {"chat_jid": "nobody@g.us"},
    "get_direct_chat_by_contact": {"sender_phone_number": "999"},
    "get_last_interaction": {"jid": "nobody@s.whatsapp.net"},
    "list_messages": {"query": "no such text"},
}


@pytest.fixture
def server(tmp_path, monkeypatch):
    db = tmp_path / "messages.db"
    conn = sqlite3.connect(db)
    conn.executescript(f"""
        CREATE TABLE chats (jid TEXT PRIMARY KEY, name TEXT, last_message_time TIMESTAMP);
        CREATE TABLE messages (
            id TEXT, chat_jid TEXT, sender TEXT, content TEXT, timestamp TIMESTAMP,
            is_from_me BOOLEAN, media_type TEXT, filename TEXT, url TEXT,
            media_key BLOB, file_sha256 BLOB, file_enc_sha256 BLOB, file_length INTEGER,
            quoted_message_id TEXT, PRIMARY KEY (id, chat_jid));
        INSERT INTO chats VALUES ('{CHAT}', 'Huis', '2026-11-04 10:02:00+00:00');
        INSERT INTO chats VALUES ('{DM}', 'Sien', '2026-11-04 09:00:00+00:00');
        INSERT INTO messages (id, chat_jid, sender, content, timestamp, is_from_me)
            VALUES ('M1', '{CHAT}', '31612345678', 'Eten jullie mee?', '2026-11-04 10:00:00+00:00', 0);
        INSERT INTO messages (id, chat_jid, sender, content, timestamp, is_from_me, quoted_message_id)
            VALUES ('M2', '{CHAT}', '31612345678', 'Ja!', '2026-11-04 10:01:00+00:00', 0, 'M1');
        INSERT INTO messages (id, chat_jid, sender, content, timestamp, is_from_me)
            VALUES ('M3', '{DM}', '31612345678', 'Hoi', '2026-11-04 09:00:00+00:00', 0);
    """)
    conn.commit()
    conn.close()

    media = tmp_path / "media"
    media.mkdir()
    (media / "photo.jpg").write_bytes(b"jpg")
    (media / "voice.ogg").write_bytes(b"ogg")
    CALLS["send_file"]["media_path"] = str(media / "photo.jpg")
    CALLS["send_audio_message"]["media_path"] = str(media / "voice.ogg")

    monkeypatch.setattr(whatsapp, "MESSAGES_DB_PATH", str(db))
    monkeypatch.setattr(whatsapp, "WHATSAPP_DB_PATH", str(tmp_path / "missing.db"))
    monkeypatch.setattr(whatsapp, "MEDIA_DIR", str(media))
    monkeypatch.setattr(whatsapp, "BRIDGE_TOKEN", "t" * 32)
    bridge = Mock(status_code=200, json=lambda: {"success": True, "message": "ok", "path": "/tmp/x.jpg"})
    monkeypatch.setattr(whatsapp.requests, "post", Mock(return_value=bridge))

    monkeypatch.delenv("WHATSAPP_MCP_READ_ONLY", raising=False)
    sys.modules.pop("main", None)
    import main
    return main.mcp


def call(server, name, args):
    result = asyncio.run(server.call_tool(name, args))
    text = " ".join(getattr(c, "text", "") for c in result.content)
    assert not result.is_error, f"{name} failed: {text}"
    return text


def test_every_tool_is_covered(server):
    tools = {t.name for t in asyncio.run(server.list_tools())}
    assert tools == set(CALLS)


@pytest.mark.parametrize("name", sorted(CALLS))
def test_tool_call_succeeds(server, name):
    call(server, name, CALLS[name])


@pytest.mark.parametrize("name", sorted(EMPTY_CALLS))
def test_tool_call_with_no_result_succeeds(server, name):
    call(server, name, EMPTY_CALLS[name])


def test_results_carry_the_data(server):
    assert "(reply to M1)" in call(server, "list_messages", {"limit": 5})
    assert "Huis" in call(server, "list_chats", {"limit": 5})
    assert "Sien" in call(server, "get_chat", {"chat_jid": DM})


SEND = {"send_message", "send_file", "send_audio_message", "react_to_message"}


def test_every_tool_has_annotations(server):
    for tool in asyncio.run(server.list_tools()):
        a = tool.annotations
        assert a is not None and a.title, f"{tool.name} has no annotations"
        if tool.name in SEND:
            assert a.read_only_hint is False and a.destructive_hint is True, tool.name
        elif tool.name == "download_media":
            assert a.read_only_hint is False and a.destructive_hint is False, tool.name
        else:
            assert a.read_only_hint is True, tool.name
