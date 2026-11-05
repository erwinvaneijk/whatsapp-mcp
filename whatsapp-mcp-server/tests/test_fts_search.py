"""list_messages(query=...) uses the FTS5 index only while the bridge keeps it
up to date (its triggers exist), and falls back to LIKE otherwise."""

import sqlite3
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import whatsapp

SCHEMA = """
CREATE TABLE chats (jid TEXT PRIMARY KEY, name TEXT, last_message_time TIMESTAMP);
CREATE TABLE messages (
    id TEXT, chat_jid TEXT, sender TEXT, content TEXT, timestamp TIMESTAMP,
    is_from_me BOOLEAN, media_type TEXT, filename TEXT, url TEXT,
    media_key BLOB, file_sha256 BLOB, file_enc_sha256 BLOB, file_length INTEGER,
    PRIMARY KEY (id, chat_jid)
);
"""
FTS = """
CREATE VIRTUAL TABLE messages_fts USING fts5(content, content='messages', content_rowid='rowid');
"""
TRIGGER = """
CREATE TRIGGER messages_ai AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts(rowid, content) VALUES (new.rowid, new.content);
END;
"""


def insert(conn, msg_id, content):
    conn.execute(
        "INSERT INTO messages (id, chat_jid, sender, content, timestamp, is_from_me) "
        "VALUES (?, 'c@s.whatsapp.net', '1', ?, '2026-11-04 10:00:00+00:00', 0)",
        (msg_id, content),
    )


class FTSSearchTests(unittest.TestCase):
    def search(self, setup, query):
        with tempfile.TemporaryDirectory() as d:
            db = str(Path(d) / "messages.db")
            conn = sqlite3.connect(db)
            conn.executescript(SCHEMA)
            conn.execute("INSERT INTO chats VALUES ('c@s.whatsapp.net', 'Chat', '2026-11-04')")
            setup(conn)
            conn.commit()
            conn.close()
            with (
                patch.object(whatsapp, "MESSAGES_DB_PATH", db),
                patch.object(whatsapp, "WHATSAPP_DB_PATH", str(Path(d) / "missing.db")),
            ):
                return str(whatsapp.list_messages(query=query, include_context=False))

    def test_fts_prefix_match(self):
        def setup(conn):
            conn.executescript(FTS + TRIGGER)
            insert(conn, "m1", "team meeting at noon")

        self.assertIn("team meeting", self.search(setup, "meet"))

    def test_fts_query_syntax_is_literal(self):
        def setup(conn):
            conn.executescript(FTS + TRIGGER)
            insert(conn, "m1", "cats AND dogs")

        # Unquoted, AND/NEAR or a stray " would be FTS5 syntax or an error.
        self.assertIn("cats AND dogs", self.search(setup, "cats AND dogs"))
        self.search(setup, 'say "hi')  # must not raise

    def test_stale_index_is_not_used(self):
        # A bridge run without FTS5 dropped the triggers; rows added since then
        # are missing from the index, so the search must use LIKE.
        def setup(conn):
            conn.executescript(FTS)
            insert(conn, "m1", "added after the triggers were dropped")

        self.assertIn("triggers were dropped", self.search(setup, "dropped"))

    def test_no_fts_table_uses_like(self):
        def setup(conn):
            insert(conn, "m1", "plain substring search")

        self.assertIn("plain substring", self.search(setup, "substr"))


if __name__ == "__main__":
    unittest.main()
