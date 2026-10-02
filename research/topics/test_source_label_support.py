import unittest
from scipy import sparse
from source_label_support import Unit,labels
from stable_lexical_frame import State
from supported_labels import prepare_background,label,Profile


class OriginalSupport(unittest.TestCase):
    def setUp(self):
        self.terms=['database','timeout','worker']
        self.counts=sparse.csr_matrix([[2,1,0],[1,1,0],[0,0,100]])
        self.owners=[0,0,1];self.ids=[7,9,11];self.states=[State.READY]*2
        self.background=prepare_background(sparse.csr_matrix([[3,2,0],[0,0,100]]+[[0,0,0]]*8),self.terms)

    def run_labels(self,unit,**changes):
        args=dict(groups=[[0],[1]],counts=self.counts,owners=self.owners,source_ids=self.ids,
            terms=self.terms,states=self.states,background=self.background,unit=unit);args.update(changes)
        return labels(**args)

    def test_multiple_original_messages_recover_single_block_labels(self):
        self.assertEqual(self.run_labels(Unit.BLOCK),[[],[]])
        candidate=self.run_labels(Unit.MESSAGE)
        self.assertEqual({t['term'] for t in candidate[0]}, {'database','timeout'})
        self.assertTrue(all(t['member_sources']==2 and t['member_blocks']==1 for t in candidate[0]))
        self.assertEqual(candidate[1],[]) # 100 repetitions in one source are not two sources.

    def test_block_control_replays_existing_score_and_rank(self):
        owners=[0,1,2];states=[State.READY]*3;groups=[[0,1],[2]]
        old=label(groups,self.counts,self.terms,states,self.background,Profile.DISTINCTIVE)
        new=self.run_labels(Unit.BLOCK,owners=owners,states=states,groups=groups)
        self.assertEqual([{k:v for k,v in t.items() if k!='member_sources'} for t in new[0]],old[0].labels)

    def test_duplicate_context_id_or_incomplete_ownership_rejected(self):
        for changes in [dict(source_ids=[7,7,11]),dict(groups=[[0]]),dict(owners=[0,0,-1]),dict(owners=[0,.5,1])]:
            with self.assertRaises(ValueError):self.run_labels(Unit.MESSAGE,**changes)

    def test_unready_source_and_fallback_block_grant_no_support(self):
        self.assertEqual(self.run_labels(Unit.MESSAGE,owners=[0,-1,1]),[[],[]])
        self.assertEqual(self.run_labels(Unit.MESSAGE,states=[State.FALLBACK,State.READY]),[[],[]])


if __name__=='__main__':unittest.main()
