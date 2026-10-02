import os
from pathlib import Path
import unittest

import numpy as np

from links_reference import load
from links_state import snapshot
from links_trial import run


@unittest.skipUnless(os.environ.get('MASH_TOPIC_LINKS_REFERENCE'),'explicit pinned author reference required')
class LinksTrialTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):cls.reference=load(Path(os.environ['MASH_TOPIC_LINKS_REFERENCE']))

    def model(self):
        return self.reference.LinksClusterer(tc=.5,ts=.8,tp=1.,normalize_input=False)

    def test_capacity_stops_before_mutation_and_source_loss(self):
        r=run(self.reference,np.eye(3),maximum=1)
        self.assertFalse(r.complete);self.assertEqual(r.attempted,1)
        self.assertEqual(r.model.n_samples_seen_,1)
        np.testing.assert_array_equal(snapshot(r.model,np.eye(3),r.arrivals).labels,[0,-1,-1])

    def test_tie_and_reverse_source_ownership(self):
        x=np.array([[1.,0.],[0.,1.],[1/np.sqrt(2),1/np.sqrt(2)]])
        r=run(self.reference,x)
        self.assertEqual(r.events[-1]['nearest'],0)
        self.assertEqual(len(snapshot(r.model,x,r.arrivals).members),3)
        backwards=run(self.reference,np.eye(3),reverse=True)
        np.testing.assert_array_equal(snapshot(backwards.model,np.eye(3),backwards.arrivals).labels,[2,1,0])

    def test_actual_merge_retains_every_source(self):
        x=np.array([[1.,0.],[.7,np.sqrt(.51)],[np.cos(.35),np.sin(.35)]])
        m=self.model()
        for i,row in enumerate(x):
            m.predict_next(row);snapshot(m,x,list(range(i+1)))
            if i==1:self.assertEqual(m.n_subclusters_,2)
        self.assertEqual(m.n_subclusters_,1)
        np.testing.assert_array_equal(snapshot(m,x,[0,1,2]).counts,[3])

    def test_split_changes_final_labels_but_keeps_arrival_answers(self):
        a=np.array([1.,0.]);b=np.array([.6,.8]);x=np.stack([a,b]+[b]*20+[a]*20)
        m=self.model()
        for i,row in enumerate(x):m.predict_next(row);snapshot(m,x,list(range(i+1)))
        self.assertEqual(m.n_clusters_,2)
        self.assertEqual(m.get_online_labels()[0],m.get_online_labels()[1])
        self.assertNotEqual(m.get_final_labels()[0],m.get_final_labels()[1])

    def test_repair_reconnects_severed_component(self):
        # Explicit internal transition fixture: edge 0--1 becomes invalid,
        # while 0--2 is a valid alternate and 1--2 remains valid.
        x=np.array([[1.,0.],[.2,np.sqrt(.96)],[np.cos(.8),np.sin(.8)]])
        m=self.model();cid=m._alloc_cluster_id()
        for i,row in enumerate(x):m._add_subcluster(row,i,cid)
        m.n_samples_seen_=3;m.n_features_in_=2;m._online_labels=[cid]*3
        m._graph.add_edge(0,1);m._graph.add_edge(1,2)
        m._update_clusters(0)
        self.assertFalse(m._graph.has_edge(0,1));self.assertTrue(m._graph.has_edge(0,2))
        self.assertEqual(m.n_clusters_,1);snapshot(m,x,[0,1,2])

    def test_corrupted_source_ownership_is_rejected(self):
        x=np.eye(2);r=run(self.reference,x)
        r.model._subclusters[0].vector_indices.append(0)
        with self.assertRaises(ValueError):snapshot(r.model,x,r.arrivals)


if __name__=='__main__':unittest.main()
