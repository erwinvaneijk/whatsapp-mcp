import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

import whatsapp


class SendFileCaptionTests(unittest.TestCase):
    def send(self, *args):
        with tempfile.TemporaryDirectory() as d:
            photo = Path(d) / "photo.jpg"
            photo.write_bytes(b"jpg")
            post = Mock(
                return_value=Mock(status_code=200, json=lambda: {"success": True, "message": "ok"})
            )
            with (
                patch.object(whatsapp, "MEDIA_DIR", d),
                patch.object(whatsapp, "BRIDGE_TOKEN", "t" * 32),
                patch.object(whatsapp.requests, "post", post),
            ):
                ok, msg = whatsapp.send_file("123", str(photo), *args)
            self.assertTrue(ok, msg)
            return post.call_args.kwargs["json"]

    def test_caption_is_sent_as_message(self):
        self.assertEqual(self.send("Look at this")["message"], "Look at this")

    def test_no_caption_sends_empty_message(self):
        self.assertEqual(self.send()["message"], "")


if __name__ == "__main__":
    unittest.main()
