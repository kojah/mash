from unittest import TestCase
import numpy as np
from topic_kernel import route,combine
from topic_stream import Source,Stream
from topic_refresh import refresh


class KernelTopicTests(TestCase):
    def test_dense_kernel_matches_centroid_routing_and_leave_one_out(self):
        vectors=np.array([[1.,0.],[0.,1.],[2**-.5,2**-.5],[-1.,0.]])
        for order in [list(range(4)),list(reversed(range(4)))]:
            model=Stream(dimensions=2)
            for i in order:model.upsert(Source(str(i),'t','provider','fixture',vectors[i]))
            expected=refresh(model.capture())['memberships'];actual=route(vectors@vectors.T,order)
            self.assertEqual(actual.anchors,tuple(model.records[str(i)].anchor for i in range(4)))
            for i,tags in enumerate(actual.memberships):
                self.assertEqual([f'leaf-{j}' for j,_ in tags],[t['topic'] for t in expected[str(i)]])
                np.testing.assert_allclose([score for _,score in tags],[t['score'] for t in expected[str(i)]],rtol=0,atol=1e-12)

    def test_zero_lexical_rows_are_explicit_not_forced_into_topics(self):
        dense=np.eye(3);lexical=np.diag([1.,0.,1.])
        only=route(lexical,[0,1,2]);self.assertEqual(only.unroutable,1)
        self.assertIsNone(only.anchors[1]);self.assertEqual(only.memberships[1],())
        hybrid=combine(dense,lexical)
        np.testing.assert_allclose(np.diag(hybrid),[1,1,1],rtol=0,atol=1e-12)
        self.assertEqual(route(hybrid,[0,1,2]).unroutable,0)

    def test_scope_and_nonfinite_inputs_reject(self):
        for kernel,order in [(np.eye(2),[0,0]),(np.ones((2,3)),[0,1]),
                              (np.array([[1,np.nan],[np.nan,1]]),[0,1]),(np.eye(2)*2,[0,1])]:
            with self.assertRaises(ValueError):route(kernel,order)
        with self.assertRaises(ValueError):combine(np.eye(2),np.eye(2),float('nan'))
