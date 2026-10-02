import unittest
import numpy as np
from scipy import sparse
from label_noise import Policy,clean


class LabelNoiseTests(unittest.TestCase):
    def test_exact_markers_preserve_plain_words_and_unicode_offsets(self):
        text='é {disfmarker} disfmarker {vocalsound}'
        c=sparse.csr_matrix([[2,1]],dtype=np.int64)
        variants,ledger=clean(c,[text],['disfmarker','vocalsound'])
        self.assertEqual(variants[Policy.MARKERS].toarray().tolist(),[[1,0]])
        first=ledger[0];self.assertEqual(text[first['char_start']:first['char_end']],'{disfmarker}')
        self.assertEqual(text.encode()[first['utf8_start']:first['utf8_end']],b'{disfmarker}')
        self.assertEqual(first['utf8_start'],first['char_start']+1)
        self.assertEqual(c.toarray().tolist(),[[2,1]])

    def test_case_sensitive_marker_rule_and_repeated_contributions(self):
        text='{Disfmarker} {disfmarker} {disfmarker}'
        variants,ledger=clean(sparse.csr_matrix([[3]],dtype=np.int64),[text],['disfmarker'])
        self.assertEqual(len(ledger),2);self.assertEqual(variants[Policy.MARKERS].toarray().tolist(),[[1]])

    def test_vocabulary_policy_only_changes_labels_and_retains_empty_rows(self):
        c=sparse.csr_matrix([[2,1,3],[0,0,1]],dtype=np.int64)
        variants,ledger=clean(c,['the database yeah yeah yeah','yeah'],['database','the','yeah'])
        self.assertEqual(variants[Policy.VOCABULARY].toarray().tolist(),[[2,0,0],[0,0,0]])
        self.assertEqual(variants[Policy.RAW].shape,variants[Policy.VOCABULARY].shape)
        self.assertEqual(ledger,[])

    def test_mismatched_or_missing_marker_counts_fail_closed(self):
        with self.assertRaises(ValueError):clean(sparse.csr_matrix([[0]],dtype=np.int64),['{disfmarker}'],['disfmarker'])
        with self.assertRaises(ValueError):clean(sparse.csr_matrix([[1]],dtype=np.int64),['{disfmarker}'],['other'])
        with self.assertRaises(ValueError):clean(sparse.csr_matrix([[1.]]),['text'],['text'])
