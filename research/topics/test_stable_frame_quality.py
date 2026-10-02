import unittest
import numpy as np
from scipy import sparse
from captured_hierarchy import temporal,Baseline
from stable_frame_quality import identity_frame,cosine_errors,label_cut,label_examples
from stable_lexical_frame import Policy,State,encode,bucket


class Quality(unittest.TestCase):
    def test_identity_fallback_empty_and_input_preservation(self):
        c=sparse.csr_matrix([[0,2],[3,0],[0,0]]);before=c.copy()
        v,states=identity_frame(c,['database','yeah'],Policy.FILTERED)
        self.assertEqual(states,[State.FALLBACK,State.READY,State.EMPTY])
        np.testing.assert_array_equal(v.toarray(),[[0,1],[1,0],[0,0]])
        self.assertEqual((c-before).nnz,0)
        with self.assertRaises(ValueError):identity_frame(c,['yeah','database'],Policy.RAW)

    def test_collision_error_is_not_silently_zero(self):
        seen={};pair=None
        for i in range(2000):
            t=f'term{i}';b=bucket(t)
            if b in seen:pair=sorted([seen[b],t]);break
            seen[b]=t
        self.assertIsNotNone(pair)
        h=sparse.vstack([encode({t:1},Policy.RAW).direction for t in pair],format='csr')
        e=cosine_errors(h,sparse.eye(2,format='csr'))
        self.assertEqual(e['pairs'],1);self.assertAlmostEqual(e['maximum_absolute'],1)

    def test_cut_full_ownership_and_does_not_depend_on_child_order(self):
        t=temporal(13,Baseline.BALANCED);groups=label_cut(t)
        self.assertEqual(len(groups),8);self.assertEqual(sorted(sum(groups,[])),list(range(13)))
        t.children.update({n:tuple(reversed(c)) for n,c in t.children.items()})
        self.assertEqual(label_cut(t),groups)

    def test_label_fallback_abstains_with_original_representative(self):
        t=temporal(2,Baseline.BALANCED);c=sparse.csr_matrix([[0,4],[3,0]])
        v,states=identity_frame(c,['database','yeah'],Policy.FILTERED)
        rows=label_examples(t,v,c,['database','yeah'],states)
        self.assertEqual(rows[0]['labels'],[]);self.assertEqual(rows[0]['label_eligible_blocks'],[])
        self.assertEqual(rows[0]['representative_block'],0)
        self.assertEqual(rows[1]['labels'][0]['term'],'database')


if __name__=='__main__':unittest.main()
