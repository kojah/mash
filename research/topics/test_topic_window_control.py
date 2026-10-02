import unittest
import numpy as np
from local_embeddings import quantized_direction
from topic_window_control import combine,stable_json


class WindowControlTests(unittest.TestCase):
    def test_repeat_comparison_uses_serialized_schema_and_only_excludes_runtime(self):
        self.assertEqual(stable_json({'windows':[(1,2)],'runtime':{'cpu':1}}),
                         stable_json({'windows':[[1,2]],'runtime':{'cpu':2}}))
        self.assertNotEqual(stable_json({'windows':[(1,2)],'runtime':{}}),
                            stable_json({'windows':[(1,3)],'runtime':{}}))

    def test_singleton_exact_existing_quantization(self):
        point=np.linspace(-1,1,384,dtype=np.float32)
        np.testing.assert_array_equal(combine([point],[5]),quantized_direction(point))

    def test_payload_weighting_before_quantization(self):
        a=np.zeros(384,dtype=np.float32);b=a.copy();a[0]=1;b[1]=1
        value=combine([a,b],[1,3])
        np.testing.assert_array_equal(value,quantized_direction(a*.25+b*.75))
        self.assertGreater(value[1],value[0])

    def test_invalid_outputs_fail_explicitly(self):
        for values,weights in [([],[]),([np.ones(383)],[1]),([np.ones(384)],[0]),
                               ([np.ones(384)],[float('nan')]),([np.zeros(384)],[1])]:
            with self.assertRaises(ValueError):combine(values,weights)


if __name__=='__main__':unittest.main()
