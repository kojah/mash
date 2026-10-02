from dataclasses import FrozenInstanceError
import math
import unittest
from calibrated_frame import calibrate,weighted_row
from stable_lexical_frame import Policy,State


class Calibration(unittest.TestCase):
    def test_document_frequency_not_frequency_and_unseen_weights(self):
        model=calibrate([{'database':9},{'database':1,'timeout':3}],['a','b'],Policy.RAW)
        self.assertEqual(dict(model.frequencies),{'database':2,'timeout':1})
        row=weighted_row({'database':1,'unseen':1},model,['database','timeout','unseen'])
        self.assertAlmostEqual(row.direction[0,2]/row.direction[0,0],math.log(3)+1)
        self.assertTrue(row.direction.has_sorted_indices)

    def test_frozen_model_and_new_arrivals_do_not_change_old_row(self):
        sources=[{'database':3},{'timeout':2}];model=calibrate(sources,['a','b'],Policy.RAW)
        before=weighted_row({'database':2},model);sources[0]['future']=7
        weighted_row({'brandnew':100},model)
        after=weighted_row({'database':2},model)
        self.assertEqual(before.frame_sha256,after.frame_sha256);self.assertEqual((before.direction-after.direction).nnz,0)
        with self.assertRaises(FrozenInstanceError):model.documents=3
        self.assertNotIn('future',dict(model.frequencies))

    def test_fallback_and_empty_source_retained_without_fitted_filter_changes(self):
        model=calibrate([{'yeah':2},{'database':3}],['a','b'],Policy.FILTERED)
        self.assertEqual(dict(model.frequencies),{'database':1,'yeah':1})
        self.assertEqual(weighted_row({'yeah':1},model).state,State.FALLBACK)
        self.assertIsNone(weighted_row({},model).direction)

    def test_seed_identity_and_coordinate_kind_in_fingerprint(self):
        a=calibrate([{'database':1}],['a'],Policy.RAW);b=calibrate([{'database':1}],['b'],Policy.RAW)
        self.assertNotEqual(weighted_row({'database':1},a).frame_sha256,weighted_row({'database':1},b).frame_sha256)
        self.assertNotEqual(weighted_row({'database':1},a).frame_sha256,weighted_row({'database':1},a,['database']).frame_sha256)
        for sources,ids in [([],[]),([{'database':1}]*33,list(map(str,range(33)))),([{},{}],['same','same'])]:
            with self.assertRaises(ValueError):calibrate(sources,ids,Policy.RAW)


if __name__=='__main__':unittest.main()
