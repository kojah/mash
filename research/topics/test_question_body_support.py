from pathlib import Path
import tempfile
import unittest

from question_body_support import analyze


class BodySupportTests(unittest.TestCase):
    def test_wrong_body_hash_fails_before_decode_or_title_access(self):
        with tempfile.TemporaryDirectory() as name:
            path = Path(name) / 'bad.json'
            path.write_text('not JSON')
            with self.assertRaisesRegex(ValueError, 'body capture required'):
                analyze(path, Path(name) / 'missing-title.json')


if __name__ == '__main__':
    unittest.main()
