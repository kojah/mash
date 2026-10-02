import unittest
import numpy as np
from discovery_controls import canonical,discover,Method


class DiscoveryControlsTests(unittest.TestCase):
    def test_canonical_source_partition_and_invalid_ownership(self):
        self.assertEqual(canonical([[3,1],[2,0]],4),([[0,2],[1,3]],[0,1,0,1]))
        for groups in [[[0],[0,1]],[[0],[]],[[0]],[[0],[2]]]:
            with self.assertRaises(ValueError):canonical(groups,2)

    def test_duplicates_and_separated_directions_preserve_all_sources(self):
        values=np.array([[1,0],[1,0],[0,1],[0,1]],dtype=float)
        original=values.copy();values.flags.writeable=False
        for method in Method:
            for reverse in [False,True]:
                r=discover(values,method,reverse=reverse)
                self.assertEqual(r.clusters,[[0,1],[2,3]])
                self.assertEqual(len(r.timings['insert_latencies_ms']),4)
                if method==Method.BIRCH:self.assertEqual(sum(x['samples'] for x in r.state['subclusters']),4)
                else:self.assertEqual(len(r.state['nodes']),7)
                np.testing.assert_array_equal(values,original)

    def test_invalid_vectors_are_not_silently_normalized(self):
        for method in Method:
            for matrix in [[[0,0]],[[2,0]],[[float('nan'),1]],[]]:
                with self.assertRaises(ValueError):discover(matrix,method)
