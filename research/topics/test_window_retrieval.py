import hashlib
import unittest

import numpy as np

from cqadup_source import Source
from embedding_windows import Field
from window_retrieval import maximum,ownership


class WindowRetrievalTests(unittest.TestCase):
    def test_signed_scores_and_earliest_row_ties(self):
        values,winners=maximum([-.5,-.8,-.5,.4,.7],[0,0,0,1,1],2)
        np.testing.assert_array_equal(values,[-.5,.7]);np.testing.assert_array_equal(winners,[0,4])

    def test_owner_order_need_not_be_contiguous(self):
        values,winners=maximum([.3,.8,.7,.2],[1,0,1,0],2)
        np.testing.assert_array_equal(values,[.8,.7]);np.testing.assert_array_equal(winners,[1,2])

    def test_missing_owners_and_invalid_scores_rejected(self):
        for scores,owners,n in [([.5],[0],2),([np.nan],[0],1),([2],[0],1),([.5],[.0],1),([.5],[-1],1)]:
            with self.assertRaises(ValueError):maximum(scores,owners,n)

    def fixture(self):
        d=Source('a','🙂 Title','Straße C++','hash');ledgers={}
        for field,text in [(Field.TITLE,d.title),(Field.BODY,d.text)]:
            ledgers[field]=[{'source':d.id,'source_text_sha256':hashlib.sha256(text.encode()).hexdigest(),
                            'windows':[{'source':d.id,'field':field,'ordinal':0,'start':0,'end':len(text),
                                'byte_start':0,'byte_end':len(text.encode()),'text_sha256':hashlib.sha256(text.encode()).hexdigest()}]}]
        return {'ledgers':ledgers},[d]

    def test_original_unicode_field_ownership_and_boundaries(self):
        p,docs=self.fixture();result=ownership(p,docs)
        self.assertEqual([r.field for r in result],[Field.TITLE,Field.BODY])
        self.assertGreater(result[0].byte_end,result[0].end)

    def test_gap_hash_and_identity_drift_rejected(self):
        for key,value in [('start',1),('source','other'),('byte_end',1),('text_sha256','bad')]:
            p,docs=self.fixture();p['ledgers'][Field.BODY][0]['windows'][0][key]=value
            with self.assertRaises(ValueError):ownership(p,docs)


if __name__=='__main__':unittest.main()
