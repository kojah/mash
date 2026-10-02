import unittest
import numpy as np

from dual_view import fidelity,gates


class DualViewTests(unittest.TestCase):
    def test_partial_budget_checks_never_promote_or_pass_complete_state(self):
        metrics={s:{'pairs':20,'purity_sum':11.} for s in ['all','far','crossspan']}
        record={'blocks':20,'groups':[[i,i+1] for i in range(0,20,2)],
            'fidelity':{'confirmed_passing':list(range(19)),'uncertain':[]},
            'state':{'physical_subclusters':[{'samples':2}]*10},'metrics':metrics,'within_compressed_group_crossspan_pairs':2}
        reference={s:{'purity':.554} for s in metrics};timings=[{'compression':{'fit_cpu_ms':8.,'insert_ms':[.4]*20}}]
        result=gates([record],reference,timings)
        self.assertTrue(result['retention_target_met']);self.assertTrue(result['fidelity_target_met_conservatively'])
        self.assertTrue(result['cf_cpu_target_met']);self.assertTrue(result['cf_p95_target_met'])
        self.assertEqual(result['known_numeric_state_lower_bound_bytes'],30*384*8)
        self.assertFalse(result['production_selection']);self.assertFalse(result['complete_state_gate'])

    def test_same_dense_mean_can_group_different_source_units(self):
        result=fidelity(np.asarray([[1.,0.],[1.,0.],[0.,1.]]),[[0,1],[2]])
        self.assertEqual(result['confirmed_passing'],[0,1,2]);self.assertEqual(result['cosines'],[1.,1.,1.])

    def test_uncertain_boundary_never_counts_as_confirmed_passing(self):
        x=np.asarray([[.9,np.sqrt(1-.9**2)],[.9,-np.sqrt(1-.9**2)]])
        result=fidelity(x,[[0,1]])
        self.assertEqual(result['uncertain'],[0,1]);self.assertEqual(result['confirmed_passing'],[])

    def test_cancelled_mean_and_mixed_group_fail_without_dropping_ownership(self):
        zero=fidelity(np.asarray([[1.,0.],[-1.,0.]]),[[0,1]])
        self.assertEqual(zero['cancelled_group_means'],[0]);self.assertEqual(zero['confirmed_failing'],[0,1])
        mixed=fidelity(np.eye(2),[[0,1]])
        self.assertEqual(mixed['confirmed_failing'],[0,1]);self.assertEqual(len(mixed['cosines']),2)

    def test_invalid_nonunit_and_overlapping_ownership_rejected(self):
        with self.assertRaises(ValueError):fidelity(np.zeros((1,2)),[[0]])
        with self.assertRaises(ValueError):fidelity(np.eye(2),[[0],[0]])


if __name__=='__main__':unittest.main()
