import unittest
from scipy import sparse
from semantic_units import Unit,Status,Limit
from unit_source_counts import pool


class Counts(unittest.TestCase):
    def test_original_words_and_gap_ownership(self):
        c=sparse.csr_matrix([[2,0],[0,3],[9,9],[1,1]])
        u=[Unit(0,0,4,Status.READY,Limit.ROOT),Unit(1,0,8,Status.READY,Limit.ROOT),
           Unit(2,None,0,Status.UNSUPPORTED,Limit.SOURCE),Unit(3,3,4,Status.READY,Limit.GAP)]
        original=c.copy();self.assertEqual(pool(c,u).toarray().tolist(),[[2,0],[2,3],[0,0],[1,1]])
        self.assertEqual((c-original).nnz,0)
        u[3]=Unit(3,1,12,Status.READY,Limit.GAP)
        with self.assertRaises(ValueError):pool(c,u)

    def test_empty_rows_remain_owned_and_invalid_counts_rejected(self):
        u=[Unit(0,0,2,Status.READY,Limit.ROOT)]
        self.assertEqual(pool(sparse.csr_matrix((1,2)),u).shape,(1,2))
        with self.assertRaises(ValueError):pool(sparse.csr_matrix([[.5,1]]),u)
        with self.assertRaises(ValueError):pool(sparse.csr_matrix([[1,1]]),[Unit(1,0,2,Status.READY,Limit.ROOT)])


if __name__=='__main__':unittest.main()
