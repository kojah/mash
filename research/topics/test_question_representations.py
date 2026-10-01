from pathlib import Path
import tempfile
import unittest

import numpy as np
from scipy.sparse import csr_matrix

from question_representations import View, pair_scores, read_title_capture, source_inputs
from stackoverflow_source import QuestionInput


class RepresentationTests(unittest.TestCase):
    def test_only_source_title_and_displayed_body_enter_inputs(self):
        question = QuestionInput(1, 'group', 'Rust & C++', '<p>Use <code>ffi</code>.</p><!--excluded-->', 'source')
        texts, records = source_inputs([question])
        self.assertEqual(texts['1'], 'Rust & C++\n\nUse ffi.\n')
        self.assertIsNone(records['1']['reason'])
        self.assertTrue(records['1']['pieces'])
        bad = QuestionInput(2, 'group', 'title', '<script>unsupported</script>', 'source')
        texts, records = source_inputs([bad])
        self.assertNotIn('2', texts)
        self.assertIsNotNone(records['2']['reason'])

    def test_empty_displayed_body_abstains_and_keeps_source_identity(self):
        question = QuestionInput(1, 'group', 'Image issue', '<img src="image.png"/>', 'source')
        texts, records = source_inputs([question])
        self.assertEqual(texts, {})
        self.assertEqual(records['1']['reason'], 'empty-rendered-body')
        self.assertEqual(records['1']['source_sha256'], 'source')

    def test_fixed_score_weights_and_pair_identity(self):
        title = {'a': np.asarray([1., 0.]), 'b': np.asarray([0., 1.])}
        body = {'a': np.asarray([1., 0.]), 'b': np.asarray([1., 0.])}
        scores = pair_scores([{'left': 'a', 'right': 'b'}], ['b', 'a'], title, body, csr_matrix([[1., 0.], [1., 0.]]))
        self.assertEqual(scores[View.TITLE], [0.])
        self.assertEqual(scores[View.JOINED], [1.])
        self.assertEqual(scores[View.TITLE_FUSION], [.25])
        self.assertEqual(scores[View.JOINED_FUSION], [1.])

    def test_wrong_original_capture_fails_before_decode(self):
        with tempfile.TemporaryDirectory() as name:
            path = Path(name) / 'bad.json'
            path.write_text('not JSON')
            with self.assertRaisesRegex(ValueError, 'title capture required'):
                read_title_capture(path)


if __name__ == '__main__':
    unittest.main()
