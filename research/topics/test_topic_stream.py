import copy
import json
from threading import Event
import unittest
import numpy as np
from topic_stream import Source,Stream,RouteState
from topic_refresh import refresh,terms
from topic_jobs import RefreshQueue
from topic_stream_demo import page


def source(identity,vector,text='fixture alpha',thread='thread-a',provider='provider-a'):
    return Source(identity,thread,provider,text,None if vector is None else np.array(vector,dtype=float))


class TopicStreamTests(unittest.TestCase):
    def test_one_blocked_background_job_does_not_block_routing_or_queue_captures(self):
        started = Event(); release = Event()
        def builder(capture):
            started.set()
            if not release.wait(5): raise ValueError('synthetic builder timeout')
            return refresh(capture)
        model = Stream(dimensions=2); model.upsert(source('a',[1,0])); queue = RefreshQueue(builder)
        try:
            self.assertTrue(queue.request(model)); self.assertTrue(started.wait(5))
            model.upsert(source('b',[0,1])); self.assertFalse(queue.request(model)); self.assertFalse(queue.poll(model))
            self.assertEqual(queue.started,1); self.assertEqual(len(queue.capture_ms),1)
            release.set(); queue.drain(model); self.assertEqual(queue.started,2); self.assertEqual(queue.completed,2)
            self.assertFalse(model.freshness()['pending_refresh'])
        finally: release.set(); queue.close()

    def test_inspection_page_escapes_source_titles_keywords_and_evidence(self):
        model = Stream(dimensions=2); point = source('a',[1,0],'<script>quoted</script>'); model.upsert(point); snapshot = refresh(model.capture())
        output = page(snapshot,{'thread-a':{'title':'<img src=x>'}},[point])
        self.assertNotIn('<script>',output); self.assertNotIn('<img src=x>',output); self.assertIn('&lt;script&gt;',output)
        self.assertIn('default-src',output)

    def test_cross_provider_routing_idempotence_and_input_ownership(self):
        model = Stream(dimensions=2); point = source('a',[1,0]); record = model.upsert(point)
        point.vector[0] = 0
        self.assertEqual(model.records['a'].source.vector.tolist(),[1,0])
        self.assertEqual(record,model.upsert(source('a',[1,0]))); self.assertEqual(model.revision,1)
        other = model.upsert(source('b',[1,0],provider='provider-b',thread='thread-b'))
        self.assertEqual(record.anchor,other.anchor); self.assertEqual(model.leaves[record.anchor].count,2)

    def test_capacity_and_missing_geometry_preserve_every_source(self):
        model = Stream(dimensions=2,maximum_leaves=1)
        model.upsert(source('a',[1,0])); outlier = model.upsert(source('b',[0,1])); missing = model.upsert(source('c',None))
        self.assertIs(outlier.state,RouteState.CAPACITY); self.assertIs(missing.state,RouteState.MISSING)
        snapshot = refresh(model.capture()); self.assertEqual(snapshot['diagnostics']['sources'],3)
        self.assertEqual(snapshot['memberships']['b'],[]); self.assertEqual(snapshot['memberships']['c'],[])

    def test_invalid_replacement_and_resource_budget_are_atomic(self):
        model = Stream(dimensions=2,maximum_sources=1,maximum_text_bytes=20); model.upsert(source('a',[1,0]))
        before = model.checkpoint()
        for value in [source('a',[0,0]),source('a',[1,0],text='x'*21),source('b',[1,0])]:
            with self.assertRaises(ValueError): model.upsert(value)
            self.assertEqual(before,model.checkpoint())

    def test_deferred_publication_allows_insert_lag_but_rejects_edits_deletes(self):
        model = Stream(dimensions=2); model.upsert(source('a',[1,0])); old = refresh(model.capture())
        model.upsert(source('b',[0,1])); self.assertTrue(model.publish(old))
        self.assertTrue(model.freshness()['pending_refresh']); self.assertEqual(model.freshness()['unrepresented_sources'],1)
        model.upsert(source('a',[0,1],text='replacement fixture')); self.assertIsNone(model.snapshot); self.assertFalse(model.publish(old))
        current = refresh(model.capture()); self.assertTrue(model.publish(current)); self.assertFalse(model.freshness()['pending_refresh'])
        model.delete('a'); self.assertIsNone(model.snapshot); self.assertFalse(model.publish(current))
        self.assertFalse(model.delete('absent'))

    def test_overlapping_point_and_thread_tags_inherit_all_structural_ancestors(self):
        model = Stream(dimensions=2); model.upsert(source('a',[1,0])); model.upsert(source('b',[0,1]))
        v = 2**-.5; model.upsert(source('c',[v,v],thread='thread-c'))
        snapshot = refresh(model.capture()); self.assertEqual(len(snapshot['memberships']['c']),2)
        row = next(t for t in snapshot['threads'] if t['thread']=='thread-c')
        self.assertEqual(len(row['direct_tags']),2); self.assertIn(snapshot['root'],row['ancestor_tags'])
        self.assertEqual(len(row['ancestor_tags']),len(set(row['ancestor_tags'])))

    def test_label_support_is_source_grounded_and_never_bridges_stopwords(self):
        self.assertNotIn('alpha beta',list(terms('alpha and beta')))
        model = Stream(dimensions=2)
        for i in range(2): model.upsert(source('a'+str(i),[1,0],'fixturealpha widgetalpha'))
        for i in range(4): model.upsert(source('b'+str(i),[0,1],'differentbeta '+str(i),thread='other'))
        snapshot = refresh(model.capture()); node = next(n for n in snapshot['nodes'] if n['id']=='leaf-0')
        self.assertTrue(node['labels']); self.assertTrue(all(k['source_support']>=2 for k in node['labels']))
        for keyword in node['labels']:
            self.assertTrue(all(keyword['term'] in list(terms(model.records[i].source.text)) for i in node['sources']))

    def test_restart_preserves_state_and_exact_refreshed_tree_without_inference(self):
        model = Stream(dimensions=2); model.upsert(source('a',[1,0])); model.upsert(source('b',[0,1])); model.upsert(source('c',None))
        model.upsert(source('a',[1,0],text='edited fixture alpha'))
        checkpoint = json.loads(json.dumps(model.checkpoint())); restored = Stream.restore(checkpoint)
        self.assertEqual(checkpoint,restored.checkpoint()); self.assertEqual(refresh(model.capture()),refresh(restored.capture()))
        restored.delete('b'); self.assertNotIn('leaf-1',{n['id'] for n in refresh(restored.capture())['nodes']})
        for alter in [lambda v:v['leaves'][0].update(count=7),lambda v:v['records'][0].update(anchor=999),lambda v:v.update(revision=True)]:
            corrupt = copy.deepcopy(checkpoint); alter(corrupt)
            with self.assertRaises(ValueError): Stream.restore(corrupt)

    def test_empty_stream_and_cancelled_leaf_have_no_invented_directions(self):
        model = Stream(dimensions=2); empty = refresh(model.capture()); self.assertIsNone(empty['root']); self.assertEqual(empty['threads'],[])
        # A bounded persisted aggregate may cancel after removals. Its sources
        # remain, while no centroid direction or keyword fallback is invented.
        checkpoint = {'schema_version':1,'configuration':model.checkpoint()['configuration'],'revision':2,'next_leaf':1,
            'records':[{'id':k,'thread':'t','provider':'p','text':'fixture','vector':v,'version':i,'anchor':0,'state':'assigned'}
                       for i,(k,v) in enumerate([('a',[1.,0.]),('b',[-1.,0.])],1)],'leaves':[{'id':0,'sum':[0.,0.],'count':2}]}
        restored = Stream.restore(checkpoint); snapshot = refresh(restored.capture())
        self.assertEqual(snapshot['diagnostics']['sources'],2); self.assertIsNone(snapshot['root'])


if __name__ == '__main__': unittest.main()
