import unittest
import numpy as np
from scipy import sparse
from sparse_seed_topics import discover,pair_counts,labels
from qmsum_seed_discovery import compare_capture


class SparseSeedTopicsTests(unittest.TestCase):
    def test_first_created_tie_and_fixed_source_seed(self):
        x=sparse.csr_matrix([[1.,0.],[0.,1.],[2**-.5,2**-.5],[0.,1.]])
        r=discover(x);self.assertEqual(r['assignments'],[0,1,0,1]);self.assertEqual(r['seeds'],[0,1])
        self.assertEqual(r['events'][2]['nearest'],0);self.assertEqual(r['comparisons'],5)

    def test_reverse_retains_original_row_and_empty_vector_is_rejected(self):
        r=discover(sparse.eye(3,format='csr'));reverse=discover(sparse.eye(3,format='csr'),reverse=True)
        self.assertEqual(r['seeds'],[0,1,2]);self.assertEqual(reverse['assignments'],[2,1,0])
        with self.assertRaises(ValueError):discover(sparse.csr_matrix([[0.,0.]]))

    def test_pair_contingency_retains_unknown_support(self):
        self.assertEqual(pair_counts([0,0,1,1],[0,1,1,None]),{'known_rows':3,'pairs':3,'tp':0,'fp':1,'fn':1,'tn':1})

    def test_original_counts_label_formula_and_lexical_ties(self):
        c=sparse.csr_matrix([[2,0],[0,2]],dtype=np.int64)
        r=labels(c,[0,0],np.ones(2),['a','b'])
        self.assertEqual([t['term'] for t in r[0]],['a','b'])
        self.assertAlmostEqual(r[0][0]['score'],.5*np.log(3));self.assertEqual(r[0][0]['source_count'],2)

    def test_bad_inputs_fail_closed(self):
        with self.assertRaises(ValueError):discover(sparse.csr_matrix([[2.,0.]]))
        with self.assertRaises(ValueError):pair_counts([0],[-1])
        with self.assertRaises(ValueError):labels(sparse.csr_matrix([[1]]),[1],[1.],['a'])

    def test_repeat_compares_exact_emitted_json_keys_and_only_ignores_timings(self):
        archived={'timings':{},'group_sizes':{'0':2,'1':1}}
        compare_capture(archived,{'timings':{'wall':1},'group_sizes':{0:2,1:1}})
        with self.assertRaises(ValueError):compare_capture(archived,{'timings':{},'group_sizes':{0:1,1:2}})

    def test_cutoff_equality_joins_seed(self):
        x=sparse.csr_matrix([[1.,0.],[.55,np.sqrt(1-.55**2)]])
        self.assertEqual(discover(x)['assignments'],[0,0])
