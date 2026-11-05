import unittest
from unittest.mock import Mock, patch

import whatsapp


class SendMentionsTests(unittest.TestCase):
    def payload(self, **kwargs):
        post = Mock(
            return_value=Mock(status_code=200, json=lambda: {"success": True, "message": "ok"})
        )
        with (
            patch.object(whatsapp, "BRIDGE_TOKEN", "t" * 32),
            patch.object(whatsapp.requests, "post", post),
        ):
            ok, msg = whatsapp.send_message("123@g.us", "hi @31612345678", **kwargs)
        self.assertTrue(ok, msg)
        return post.call_args.kwargs["json"]

    def test_mentions_are_sent(self):
        p = self.payload(mentions=["31612345678"])
        self.assertEqual(p["mentions"], ["31612345678"])

    def test_no_mentions_leaves_payload_unchanged(self):
        self.assertNotIn("mentions", self.payload())
        self.assertNotIn("mentions", self.payload(mentions=[]))

    def test_mentions_combine_with_reply(self):
        p = self.payload(mentions=["1"], reply_to_message_id="M1", reply_to_sender_jid="2@lid")
        self.assertEqual(
            (p["mentions"], p["reply_to_message_id"], p["reply_to_sender_jid"]),
            (["1"], "M1", "2@lid"),
        )


if __name__ == "__main__":
    unittest.main()
