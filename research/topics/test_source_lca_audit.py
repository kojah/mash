import copy
import unittest
import numpy as np
from source_lca_audit import record


def case(unknown=False):
    members=[[] for _ in range(10)]
    for i,m in zip([0,1,8,9],[[(0,0)],[] if unknown else [(0,0)],[(0,1)],[(1,0)]]):members[i]=m
    pairs=1 if unknown else 2;weight=2/3 if unknown else 1.5
    metric={'pairs':pairs,'purity_sum':weight,'purity':weight/pairs,
        'topics':[{'topic':0,'pairs':pairs,'purity_sum':weight},{'topic':1,'pairs':0,'purity_sum':0.}]}
    r={'source_ordinals':[0,1,8,9],'discovery_owners':[0,1,2,3],'original_physical_owners':[0,0,1,1],
        'tree':{'root':6,'children':{'4':[0,1],'5':[2,3],'6':[4,5]},'leaves':{str(i):i for i in range(4)}},
        'merges':[[0,1,.1,2],[2,3,.1,2],[4,5,.8,4]],'metrics':{s:copy.deepcopy(metric) for s in ['all','far','crossspan']}}
    d=np.array([[0,.1,.8,.8],[.1,0,.8,.8],[.8,.8,0,.1],[.8,.8,.1,0]])
    return r,members,d


class SourceAudit(unittest.TestCase):
    def test_original_exclusions_and_unknown_class_fraction(self):
        for unknown in [False,True]:
            r,m,d=case(unknown);a=record(r,m,d)
            self.assertEqual(a['positive_pair_scope_contributions'],3 if unknown else 6)
            self.assertEqual(a['global_minimum']['maximum_chosen_minus_global_minimum'],0)

    def test_corrupt_metric_or_population_rejected(self):
        r,m,d=case();r['metrics']['all']['pairs']=3
        with self.assertRaises(AssertionError):record(r,m,d)
        r,m,d=case();r['merges'][0][3]=3
        with self.assertRaises(AssertionError):record(r,m,d)


if __name__=='__main__':unittest.main()
