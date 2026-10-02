import unittest
import math
import numpy as np
from stable_lexical_frame import counts,encode,Policy,State,bucket,BUCKETS,fingerprint,require_frame


class StableFrame(unittest.TestCase):
    def test_future_arrivals_and_node_order_do_not_change_prior_rows(self):
        # Golden SHA256 checked independently with the system sha256sum utility:
        # 2503274576f78b237f5ae057836e7d56e72aed91cba73457059594bedd44d4c4
        self.assertEqual(bucket('cbioportal'),805)
        original=counts('cbioportal database database HTTP')
        before=encode(original,Policy.RAW);saved=before.direction.copy()
        for text in ['newterm_xyz future_repository','postgres redis','unrelated_provider_history']:
            encode(counts(text),Policy.RAW)
        after=encode(dict(reversed(list(original.items()))),Policy.RAW)
        self.assertEqual((saved-after.direction).nnz,0);self.assertEqual(before.frame_sha256,after.frame_sha256)
        self.assertEqual(original,{'cbioportal':1,'database':2,'http':1})

    def test_exact_marker_subtraction_preserves_adjacent_source_words(self):
        self.assertEqual(counts('alpha{disfmarker}beta {vocalsound} vocalsound {DISFMARKER}'),
            {'alpha':1,'beta':1,'vocalsound':1,'disfmarker':1})

    def test_filtered_empty_is_flagged_and_raw_empty_is_unassigned(self):
        row=encode(counts('okay uh'),Policy.FILTERED);raw=encode(counts('okay uh'),Policy.RAW)
        self.assertEqual(row.state,State.FALLBACK);self.assertEqual((row.direction-raw.direction).nnz,0)
        empty=encode(counts('{disfmarker}'),Policy.FILTERED)
        self.assertEqual(empty.state,State.EMPTY);self.assertIsNone(empty.direction)
        with self.assertRaises(ValueError):require_frame(row.frame_sha256,raw.frame_sha256)
        require_frame(row.frame_sha256,fingerprint(Policy.FILTERED))

    def test_collision_coalesces_per_term_weights_before_normalizing(self):
        seen={};pair=None
        for i in range(2000):
            term='term_'+str(i);b=bucket(term)
            if b in seen:pair=(seen[b],term);break
            seen[b]=term
        self.assertIsNotNone(pair)
        anchor='independent_anchor';self.assertNotEqual(bucket(anchor),bucket(pair[0]))
        row=encode({pair[0]:1,pair[1]:3,anchor:2},Policy.RAW)
        self.assertEqual(row.direction.shape,(1,BUCKETS));self.assertEqual(row.direction.nnz,2)
        ratio=float(row.direction[0,bucket(pair[0])]/row.direction[0,bucket(anchor)])
        self.assertAlmostEqual(ratio,(2+math.log(3))/(1+math.log(2)),places=12)
        self.assertAlmostEqual(float(row.direction.multiply(row.direction).sum()),1.,places=12)

    def test_bad_counts_fail_and_filtered_payload_preserves_original(self):
        for bad in [{'term':0},{'term':True},{'term':-1},{'term':1.5},{'term':2**54},
                    {'UPPER':1},{'not a token':1},{'x':1}]:
            with self.assertRaises(ValueError):encode(bad,Policy.RAW)
        original=counts('the cbioportal database');row=encode(original,Policy.FILTERED)
        self.assertEqual(row.original_counts,original);self.assertNotIn('the',row.used_counts)
        self.assertEqual(row.state,State.READY);self.assertIn('the',original)


if __name__=='__main__':unittest.main()
