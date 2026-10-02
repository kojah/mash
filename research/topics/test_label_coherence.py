import unittest

import numpy as np
from scipy import sparse

from label_coherence import diagnose, npmi, presence_statistics


class LabelCoherenceTests(unittest.TestCase):
    def test_npmi_independence_zero_joint_and_degenerate_counts(self):
        self.assertEqual(npmi(2,2,1,4),0)
        self.assertEqual(npmi(2,2,2,4),1)
        self.assertEqual(npmi(2,2,0,4),-1)
        self.assertIsNone(npmi(4,4,4,4))
        for counts in [(3,3,0,4),(0,2,0,4),(2,2,3,4)]:
            with self.assertRaises(ValueError): npmi(*counts)

    def test_document_presence_not_frequency_and_phrase_containment(self):
        x=sparse.csr_matrix([[9,2,0],[1,0,3],[0,0,1],[0,0,0]])
        df,joint=presence_statistics(x,[0,1,2])
        np.testing.assert_array_equal(df,[2,1,2])
        np.testing.assert_array_equal(joint,[[2,1,1],[1,1,0],[1,0,2]])
        result=diagnose(['api','api server','token'],{'api':0,'api server':1,'token':2},df,joint,4)
        self.assertEqual(result['zero_joint_pairs'],1)
        self.assertEqual(result['containment_pairs'],1)
        self.assertIsNone(diagnose(['api'],{'api':0},df,joint,4)['mean_npmi'])


if __name__=='__main__': unittest.main()
