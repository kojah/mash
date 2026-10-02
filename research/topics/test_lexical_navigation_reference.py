import math
import unittest
import numpy as np
from scipy import sparse
from lexical_navigation_reference import features


class ScalarReference(unittest.TestCase):
    def test_original_counts_before_idf_and_normalization(self):
        result,n,error=features(sparse.csr_matrix([[2,1],[0,0]]),np.array([2.,3.]))
        first=2*(1+math.log(2));norm=math.sqrt(first*first+9)
        self.assertAlmostEqual(result[0,0],first/norm);self.assertAlmostEqual(result[0,1],3/norm)
        self.assertEqual(result[1].nnz,0);self.assertEqual(n,2);self.assertLessEqual(error,1e-12)


if __name__=='__main__':unittest.main()
