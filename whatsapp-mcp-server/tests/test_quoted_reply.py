"""Replies show which message they quote, and every message shows its ID.
Databases from before the bridge added quoted_message_id still work."""

import sqlite3
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import whatsapp

COLUMNS = """
    id TEXT, chat_jid TEXT, sender TEXT, content TEXT, timestamp TIMESTAMP,
    is_from_me BOOLEAN, media_type TEXT, filename TEXT, url TEXT,
    media_key BLOB, file_sha256 BLOB, file_enc_sha256 BLOB, file_length INTEGER{extra},
    PRIMARY KEY (id, chat_jid)
"""


class QuotedReplyTests(unittest.TestCase):
    def run_with_db(self, with_column, func):
        with tempfile.TemporaryDirectory() as d:
            db = str(Path(d) / "messages.db")
            conn = sqlite3.connect(db)
            conn.execute(
                "CREATE TABLE chats (jid TEXT PRIMARY KEY, name TEXT, last_message_time TIMESTAMP)"
            )
            conn.execute(
                "CREATE TABLE messages ("
                + COLUMNS.format(extra=", quoted_message_id TEXT" if with_column else "")
                + ")"
            )
            conn.execute(
                "INSERT INTO chats VALUES ('c@s.whatsapp.net', 'Chat', '2026-11-04 10:01:00+00:00')"
            )
            conn.execute(
                "INSERT INTO messages (id, chat_jid, sender, content, timestamp, is_from_me) "
                "VALUES ('ORIG1', 'c@s.whatsapp.net', '1', 'shall we eat?', '2026-11-04 10:00:00+00:00', 0)"
            )
            if with_column:
                conn.execute(
                    "INSERT INTO messages (id, chat_jid, sender, content, timestamp, is_from_me, quoted_message_id) "
                    "VALUES ('R1', 'c@s.whatsapp.net', '1', 'yes please', '2026-11-04 10:01:00+00:00', 1, 'ORIG1')"
                )
            else:
                conn.execute(
                    "INSERT INTO messages (id, chat_jid, sender, content, timestamp, is_from_me) "
                    "VALUES ('R1', 'c@s.whatsapp.net', '1', 'yes please', '2026-11-04 10:01:00+00:00', 1)"
                )
            conn.commit()
            conn.close()
            with (
                patch.object(whatsapp, "MESSAGES_DB_PATH", db),
                patch.object(whatsapp, "WHATSAPP_DB_PATH", str(Path(d) / "missing.db")),
            ):
                return func()

    def test_list_messages_shows_reply_and_ids(self):
        out = self.run_with_db(True, lambda: str(whatsapp.list_messages(include_context=False)))
        self.assertIn(
            "From: Me (reply to ORIG1): [Message ID: R1] <message_content>yes please</message_content>",
            out,
        )
        self.assertIn("[Message ID: ORIG1] <message_content>shall we eat?</message_content>", out)

    def test_context_and_last_interaction_show_reply(self):
        ctx = self.run_with_db(True, lambda: whatsapp.get_message_context("ORIG1", 1, 1))
        self.assertEqual(ctx.after[0].quoted_message_id, "ORIG1")
        self.assertIsNone(ctx.message.quoted_message_id)
        out = self.run_with_db(True, lambda: whatsapp.get_last_interaction("c@s.whatsapp.net"))
        self.assertIn("(reply to ORIG1)", out)

    def test_old_database_without_column(self):
        out = self.run_with_db(False, lambda: str(whatsapp.list_messages()))
        self.assertIn("yes please", out)
        self.assertNotIn("reply to", out)


if __name__ == "__main__":
    unittest.main()
