import unittest
import numpy as np
from semantic_units import aggregate, Limit, Status


class SemanticUnitsTests(unittest.TestCase):
    def test_causal_five_turn_limit_and_no_lookahead(self):
        vectors = np.eye(7);output,units = aggregate(vectors, np.ones(7,dtype=bool), np.full(7,2))
        self.assertEqual([u.start for u in units], [0,0,0,0,0,1,2])
        self.assertEqual(units[-1].limit, Limit.TURNS)
        np.testing.assert_allclose(output[3],np.array([1,1,1,1,0,0,0])/2)
        changed = vectors.copy();changed[6] = -changed[6]
        np.testing.assert_array_equal(output[:6],aggregate(changed,np.ones(7,dtype=bool),np.full(7,2))[0][:6])

    def test_unsupported_gaps_and_token_budget_keep_whole_turns(self):
        output,units = aggregate(np.eye(5),np.array([True,False,True,True,True]),np.array([2,2,256,256,512]))
        self.assertEqual(units[1].status,Status.UNSUPPORTED)
        self.assertEqual((units[2].start,units[2].limit),(2,Limit.GAP))
        self.assertEqual((units[3].start,units[3].tokens),(2,512))
        self.assertEqual((units[4].start,units[4].limit),(4,Limit.TOKENS))
        np.testing.assert_array_equal(output[1],np.zeros(5))
        np.testing.assert_array_equal(output[4],np.eye(5)[4])

    def test_cancellation_abstains_but_retains_lineage(self):
        output,units = aggregate([[1,0],[-1,0]],np.array([True,True]),np.array([2,2]))
        self.assertEqual((units[1].start,units[1].tokens,units[1].status),(0,4,Status.CANCELLED))
        np.testing.assert_array_equal(output[1],[0,0])

    def test_invalid_support_and_directions_are_rejected(self):
        for vector,ready,tokens in [([[0,0]],[True],[2]),([[1,0]],[1],[2]),([[1,0]],[True],[513]),
                                    ([[float('nan'),0]],[False],[2]),([[1,0]],[True],[2.5])]:
            with self.assertRaises(ValueError):aggregate(vector,ready,tokens)
