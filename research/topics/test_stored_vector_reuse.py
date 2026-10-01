import sqlite3
import unittest

import numpy as np

from stored_vector_reuse import messages, scores


class ReuseTests(unittest.TestCase):
    def test_secondary_match_survives_averaging_but_maximum_can_overstate_overlap(self):
        a = np.eye(3)[:2]
        b = np.eye(3)[1:]
        result = scores(a, b)
        self.assertAlmostEqual(result['mean'], .5)
        self.assertEqual(result['maximum'], 1.)
        self.assertEqual(result['bidirectional-nearest-mean'], .5)
        self.assertEqual(result, scores(b, a))

    def test_singletons_agree_and_zero_or_cancelled_means_fail(self):
        result = scores([[1., 0]], [[0., 1.]])
        self.assertEqual(set(result.values()), {0.})
        for bad in ([], [[0., 0]], [[float('nan'), 0]], [[1., 0], [-1., 0]]):
            with self.assertRaises(ValueError):
                scores(bad, [[1., 0]])

    def test_duplicate_sources_are_retained_in_counts_and_conflicts_rejected(self):
        c = sqlite3.connect(':memory:')
        c.executescript('CREATE TABLE messages(id,ordinal,search_projection,content,conversation_id,role);'
                        'CREATE TABLE message_embeddings(message_id,vector,generation,dimensions,norm);')
        raw = bytes([1] + [0] * 383)
        for identity in ('a', 'b'):
            c.execute('INSERT INTO messages VALUES(?,1,NULL,?,"t","user")', (identity, 'same'))
            c.execute('INSERT INTO message_embeddings VALUES(?,?,"g",384,1)', (identity, raw))
        values, duplicates = messages(c, 't', 'g')
        self.assertEqual((len(values), duplicates), (1, 1))
        c.execute('UPDATE message_embeddings SET vector=? WHERE message_id="b"', (bytes([0, 1] + [0] * 382),))
        with self.assertRaisesRegex(ValueError, 'conflicting'):
            messages(c, 't', 'g')
        with self.assertRaisesRegex(ValueError, 'drift'):
            messages(c, 't', 'changed')
        c.close()


if __name__ == '__main__':
    unittest.main()
