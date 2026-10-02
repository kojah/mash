from dataclasses import asdict
import unittest
import numpy as np
from scipy import sparse
from online_tree import Tree,Mode,Linkage
from sparse_average_tree import SparseAverageTree


class SparseLinkage(unittest.TestCase):
    def test_tiny_finite_norm_matches_dense_and_does_not_mutate_input(self):
        v=np.array([1e-18,0.,0.]);source=sparse.csr_matrix(v.reshape(1,-1));before=source.copy()
        a=SparseAverageTree(Mode.ROTATE);b=Tree(Mode.ROTATE,linkage=Linkage.AVERAGE)
        a.insert('tiny',source);b.insert('tiny',v);a.validate();b.validate()
        # CSR reciprocal scaling and dense division can differ by one ULP.
        np.testing.assert_allclose(a.root.total.toarray()[0],b.root.total,rtol=0,atol=1e-12)
        self.assertEqual((source-before).nnz,0)

    def test_invalid_norm_fails_before_tree_mutation(self):
        a=SparseAverageTree(Mode.ROTATE);a.insert('valid',sparse.csr_matrix([[1.,0.]]))
        root=a.root;statistics=asdict(a.statistics)
        for v in [sparse.csr_matrix([[1e308,0.]]),sparse.csr_matrix([[1e-200,0.]]),
                  sparse.csr_matrix(([1.,-1.],([0,0],[0,0])),shape=(1,2))]:
            with self.assertRaises(ValueError):a.insert('rejected',v)
            self.assertIs(a.root,root);self.assertEqual(a.serial,1)
            self.assertEqual(a.identities,['valid']);self.assertEqual(asdict(a.statistics),statistics);a.validate()

    def test_continuous_nonboundary_dense_prefix_parity(self):
        vectors=np.random.default_rng(20261002).normal(size=(18,12))
        for mode in Mode:
            a=SparseAverageTree(mode);b=Tree(mode,linkage=Linkage.AVERAGE)
            for i,v in enumerate(vectors):
                a.insert(str(i),sparse.csr_matrix(v.reshape(1,-1)));b.insert(str(i),v);a.validate();b.validate()
                self.assertEqual(asdict(a.statistics),asdict(b.statistics))
                self.assertEqual({k:a.leaf_indices(n) for k,n in a.nodes.items()},{k:b.leaf_indices(n) for k,n in b.nodes.items()})

    def test_dense_parity_every_prefix_and_pair_mean(self):
        # Binary rational unit directions avoid accidental BLAS/CSR tie changes.
        vectors=np.eye(8)[[0,3,0,5,3,7,5,0,7,3]]
        for mode in Mode:
            for order in [list(range(len(vectors))),list(range(len(vectors)-1,-1,-1))]:
                a=SparseAverageTree(mode);b=Tree(mode,linkage=Linkage.AVERAGE)
                for i in order:
                    a.insert(str(i),sparse.csr_matrix(vectors[i:i+1]));b.insert(str(i),vectors[i]);a.validate();b.validate()
                    self.assertEqual(asdict(a.statistics),asdict(b.statistics));self.assertEqual(a.identities,b.identities)
                    self.assertEqual({k:a.leaf_indices(v) for k,v in a.nodes.items()},{k:b.leaf_indices(v) for k,v in b.nodes.items()})
                    for node in a.nodes.values():
                        np.testing.assert_allclose(node.total.toarray()[0],b.nodes[node.serial].total,atol=1e-12)
                for x in a.nodes.values():
                    for y in a.nodes.values():
                        xs=[order[i] for i in a.leaf_indices(x)];ys=[order[i] for i in a.leaf_indices(y)]
                        self.assertAlmostEqual(a.score(x,y),float(np.mean(vectors[xs]@vectors[ys].T)),places=12)

    def test_sparse_high_dimension_and_invalid_input(self):
        a=SparseAverageTree(Mode.GRINCH,maximum_leaves=2)
        v=sparse.csr_matrix(([1.],([0],[18000])),shape=(1,18316))
        a.insert('one',v);a.insert('two',v);a.validate();self.assertEqual(a.root.count,2)
        with self.assertRaises(ValueError):a.insert('three',v)
        for bad in [sparse.csr_matrix((1,18316)),sparse.csr_matrix([[np.nan]]),sparse.csr_matrix(np.ones((2,3)))]:
            with self.assertRaises(ValueError):SparseAverageTree(Mode.GREEDY).insert('x',bad)

    def test_validation_detects_restructure_aggregate_and_source_drift(self):
        a=SparseAverageTree(Mode.GRINCH)
        for i in range(4):a.insert(str(i),sparse.csr_matrix(np.eye(4)[i:i+1]))
        a.validate();a.root.count+=1
        with self.assertRaises(ValueError):a.validate()
        a.root.count-=1;a.matrix[0,0]=.5
        with self.assertRaises(ValueError):a.validate()


if __name__=='__main__':unittest.main()
