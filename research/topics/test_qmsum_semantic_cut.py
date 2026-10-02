import unittest
from qmsum_semantic_cut import summarize


class Summary(unittest.TestCase):
    def test_micro_aggregation_keeps_abstained_positive_support(self):
        def row(g,p,t):
            return {**{s:{'gold_pairs':g,'predicted_pairs':p,'true_positive_pairs':t} for s in ['all','far','crossspan']},
                'coverage':{'ready_sources':4,'groups':int(p>0),'group_sizes':[2] if p else []}}
        result=summarize([row(9,1,1),row(1,0,0)])
        self.assertEqual(result['all']['precision'],1)
        self.assertEqual(result['all']['recall'],.1)
        self.assertEqual(result['all']['gold_pairs'],10)
        self.assertEqual(result['coverage']['ready_sources'],8)
        self.assertEqual(result['group_sizes'],[2])

    def test_no_predictions_are_undefined_precision(self):
        self.assertIsNone(summarize([])['all']['precision'])
        self.assertIsNone(summarize([])['all']['recall'])


if __name__=='__main__':unittest.main()
