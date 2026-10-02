import json
import math
from unittest import TestCase
from unittest.mock import patch
import numpy as np
from topic_stream import Source,Stream
from topic_flat import refresh_flat
from topic_refresh import refresh
from topic_jobs import RefreshQueue


class FlatTopicTests(TestCase):
    def model(self):
        model=Stream(dimensions=2)
        for identity,vector,text,thread,provider in [
            ('a',[1,0],'alpha widgets','t-a','claude'),('a2',[1,0],'alpha widgets','t-a','claude'),
            ('b',[0,1],'beta sockets','t-b','codex'),('b2',[0,1],'beta sockets','t-b','codex'),
            ('c',[2**-.5,2**-.5],'alpha widgets beta sockets','t-c','codex'),
            ('missing',None,'','t-d','claude')]:
            model.upsert(Source(identity,thread,provider,text,None if vector is None else np.array(vector)))
        return model

    def test_flat_assignments_match_control_without_calling_hierarchy(self):
        model=self.model();control=refresh(model.capture())
        with patch('topic_refresh.average',side_effect=AssertionError('tree must not run')):
            flat=refresh_flat(model.capture())
        self.assertEqual(flat['memberships'],control['memberships'])
        self.assertEqual(flat['versions'],control['versions'])
        self.assertEqual(len(flat['memberships']['c']),2)
        self.assertEqual(flat['memberships']['missing'],[])
        self.assertEqual(flat['diagnostics']['missing_vectors'],1)
        self.assertNotIn('root',flat);self.assertNotIn('nodes',flat)
        self.assertTrue(all('parent' not in t and 'children' not in t for t in flat['topics']))
        thread=next(t for t in flat['threads'] if t['thread']=='t-c')
        self.assertEqual(set(thread['tags']),{t['topic'] for t in flat['memberships']['c']})
        self.assertNotIn('ancestor_tags',thread)

    def test_flat_label_support_restart_edit_and_delete(self):
        model=self.model();initial=refresh_flat(model.capture())
        restored=Stream.restore(json.loads(json.dumps(model.checkpoint())))
        self.assertEqual(initial,refresh_flat(restored.capture()))
        model.upsert(Source('a','t-a','claude','updated sockets',np.array([0.,1.])))
        self.assertFalse(model.publish(initial))
        current=refresh_flat(model.capture());self.assertTrue(model.publish(current))
        model.delete('b');self.assertFalse(model.publish(current))
        deleted=refresh_flat(model.capture())
        self.assertNotIn('b',deleted['versions'])
        self.assertTrue(all('b' not in topic['sources'] for topic in deleted['topics']))
        for topic in initial['topics']:
            self.assertTrue(all(label['source_support']>=2 for label in topic['labels']))
        queue=RefreshQueue(refresh_flat)
        try:queue.drain(model);self.assertEqual(model.snapshot,deleted)
        finally:queue.close()

    def test_empty_and_invalid_parameters(self):
        model=Stream(dimensions=2);snapshot=refresh_flat(model.capture())
        self.assertEqual(snapshot['topics'],[]);self.assertEqual(snapshot['threads'],[])
        for kwargs in [{'threshold':True},{'relative':float('nan')},{'maximum_tags':False}]:
            with self.assertRaises(ValueError):refresh_flat(model.capture(),**kwargs)

    def test_labels_use_only_flat_class_statistics(self):
        model=Stream(dimensions=3)
        for group,text in enumerate(['alpha, widgets','beta, sockets','gamma, caches']):
            vector=np.eye(3)[group]
            for occurrence in range(2):
                model.upsert(Source(f'{group}-{occurrence}',str(group),'provider',text,vector))
        snapshot=refresh_flat(model.capture())
        # Three disjoint classes, four terms each: class TF=.5, mean length=4,
        # global class frequency=2 and source DF=2 of six original sources.
        expected=.5*math.log1p(4/2)*(math.log(7/3)+1)
        self.assertTrue(all(topic['label_support']=='supported' for topic in snapshot['topics']))
        for topic in snapshot['topics']:
            self.assertEqual(len(topic['labels']),2)
            for label in topic['labels']:
                self.assertEqual(label['source_support'],2)
                self.assertAlmostEqual(label['score'],expected,places=12)
