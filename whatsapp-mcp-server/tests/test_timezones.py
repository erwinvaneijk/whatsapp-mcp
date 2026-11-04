"""Times are compared and sorted as moments, not as text (#146).

The bridge stores each time with its UTC offset (+01:00 in winter, +02:00 in
summer). As text, "after 17:00Z" would compare against local clock times and
be off by one or two hours, and the hour that repeats when summer time ends
would sort wrongly. The server compares julianday() values instead.
"""

import os
import re
import sqlite3
import sys
import time
from datetime import datetime, timedelta, timezone

import pytest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

import whatsapp  # noqa: E402

CHAT = "123@g.us"
# (id, stored time) in the bridge's format
ROWS = [
    ("W1", "2026-10-04 18:30:00+02:00"),  # 16:30Z
    ("W2", "2026-10-04 19:30:00+02:00"),  # 17:30Z
    ("W3", "2026-10-04 20:30:00+02:00"),  # 18:30Z
    ("W4", "2026-10-04 21:30:00+02:00"),  # 19:30Z
    # When summer time ends, 02:00-03:00 happens twice:
    ("D1", "2026-10-25 02:30:00+02:00"),  # 00:30Z, earlier
    ("D2", "2026-10-25 02:15:00+01:00"),  # 01:15Z, later, though "02:15" < "02:30" as text
    ("U1", "2026-11-04 20:00:00.250+00:00"),  # stored by a bridge running in UTC
]
# The indexes NewMessageStore creates (whatsapp-bridge/main.go).
BRIDGE_INDEXES = """
CREATE INDEX idx_messages_chat_moment ON messages(chat_jid, julianday(timestamp));
CREATE INDEX idx_messages_sender_moment ON messages(sender, julianday(timestamp));
CREATE INDEX idx_messages_moment ON messages(julianday(timestamp));
CREATE INDEX idx_chats_last_message_moment ON chats(julianday(last_message_time));
"""


@pytest.fixture
def db(tmp_path, monkeypatch):
    path = tmp_path / "messages.db"
    conn = sqlite3.connect(path)
    conn.executescript("""
        CREATE TABLE chats (jid TEXT PRIMARY KEY, name TEXT, last_message_time TIMESTAMP);
        CREATE TABLE messages (
            id TEXT, chat_jid TEXT, sender TEXT, content TEXT, timestamp TIMESTAMP,
            is_from_me BOOLEAN, media_type TEXT, filename TEXT, url TEXT,
            media_key BLOB, file_sha256 BLOB, file_enc_sha256 BLOB, file_length INTEGER,
            quoted_message_id TEXT, PRIMARY KEY (id, chat_jid));
    """ + BRIDGE_INDEXES)
    conn.execute("INSERT INTO chats VALUES (?, 'Book club', '2026-11-04 20:00:00+00:00')", (CHAT,))
    for msg_id, ts in ROWS:
        conn.execute("INSERT INTO messages (id, chat_jid, sender, content, timestamp, is_from_me) "
                     "VALUES (?, ?, '1', ?, ?, 0)", (msg_id, CHAT, "text " + msg_id, ts))
    conn.commit()
    conn.close()
    monkeypatch.setattr(whatsapp, "MESSAGES_DB_PATH", str(path))
    monkeypatch.setattr(whatsapp, "WHATSAPP_DB_PATH", str(tmp_path / "missing.db"))
    return path


def ids(**filters):
    out = whatsapp.list_messages(limit=100, include_context=False, **filters)
    return re.findall(r"\[Message ID: (\w+)\]", out)


def test_same_window_in_any_zone(db):
    dutch = ids(after="2026-10-04T19:00:00+02:00", before="2026-10-04T21:00:00+02:00")
    assert dutch == ["W3", "W2"]
    assert ids(after="2026-10-04T17:00:00Z", before="2026-10-04T19:00:00Z") == dutch
    assert ids(after="2026-10-04T10:00:00-07:00", before="2026-10-04T12:00:00-07:00") == dutch


def test_time_without_zone_is_local_time(db, monkeypatch):
    if not hasattr(time, "tzset"):
        pytest.skip("needs time.tzset (not on Windows)")
    monkeypatch.setenv("TZ", "Europe/Amsterdam")
    time.tzset()
    try:
        assert ids(after="2026-10-04T19:00:00", before="2026-10-04T21:00:00") == ["W3", "W2"]
    finally:
        monkeypatch.undo()
        time.tzset()


def test_summer_time_end_sorts_by_moment(db):
    assert ids(after="2026-10-25T00:00:00Z", before="2026-10-26T00:00:00Z") == ["D2", "D1"]


def test_fractions_and_utc_rows(db):
    assert ids(after="2026-11-04T21:00:00+01:00") == ["U1"]  # 20:00:00.250Z is after 20:00:00Z
    assert ids(after="2026-11-04T20:00:01Z") == []


def test_invalid_input_explains_the_format(db):
    with pytest.raises(ValueError, match="ISO-8601"):
        whatsapp.list_messages(after="yesterday evening")


def test_context_uses_moments(db):
    ctx = whatsapp.get_message_context("D1", before=1, after=1)
    assert [m.id for m in ctx.after] == ["D2"]
    ctx = whatsapp.get_message_context("D2", before=1, after=1)
    assert [m.id for m in ctx.before] == ["D1"]


def test_text_output_shows_the_offset(db):
    out = whatsapp.list_messages(limit=1, include_context=False, after="2026-10-04T19:15:00+02:00",
                                 before="2026-10-04T19:45:00+02:00")
    assert "[2026-10-04 19:30:00 +02:00]" in out


def test_format_time():
    t = datetime(2026, 11, 4, 22, 0, 52)
    assert whatsapp.format_time(t.replace(tzinfo=timezone(timedelta(hours=1)))) == "2026-11-04 22:00:52 +01:00"
    assert whatsapp.format_time(t.replace(tzinfo=timezone(timedelta(hours=-3, minutes=-30)))) == "2026-11-04 22:00:52 -03:30"
    assert whatsapp.format_time(t) == "2026-11-04 22:00:52"


@pytest.mark.parametrize("sql,index", [
    ("SELECT id FROM messages WHERE julianday(messages.timestamp) > julianday(?) "
     "ORDER BY julianday(messages.timestamp) DESC LIMIT 5", "idx_messages_moment"),
    ("SELECT id FROM messages WHERE messages.chat_jid = ? AND julianday(messages.timestamp) < julianday(?) "
     "ORDER BY julianday(messages.timestamp) DESC LIMIT 5", "idx_messages_chat_moment"),
])
def test_queries_use_the_bridge_indexes(db, sql, index):
    """The server's expressions must match the bridge's index expressions."""
    conn = sqlite3.connect(db)
    params = ("2026-01-01",) if sql.count("?") == 1 else (CHAT, "2026-01-01")
    plan = " ".join(row[-1] for row in conn.execute("EXPLAIN QUERY PLAN " + sql, params))
    conn.close()
    assert index in plan, plan


def test_date_alone_is_midnight_and_time_alone_is_refused(db):
    # A date alone means midnight (local time); the October 4 messages are all after it.
    assert ids(after="2026-10-04", before="2026-10-05T00:00:00Z") == ["W4", "W3", "W2", "W1"]
    # A time without a date does not say which day.
    with pytest.raises(ValueError, match="date, or date and time"):
        whatsapp.list_messages(after="21:00:00")
