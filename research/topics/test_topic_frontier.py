import unittest
from topic_frontier import frontier


class FrontierTests(unittest.TestCase):
    def inputs(self):
        def node(identity,sources,children,parent):
            return {'id':identity,'sources':sources,'children':children,'parent':parent,'labels':[{'term':'specific'},{'term':'subject'}]}
        snapshot={'root':'root','parameters':{'threshold':.55},'nodes':[
            node('a',['1','2'],[],'root'),node('b',['3','4'],[],'root'),
            node('root',['1','2','3','4'],['a','b'],None)],
            'threads':[{'thread':'thread','sources':['1','2','3','4']}]}
        checkpoint={'records':[{'id':str(i),'vector':v} for i,v in enumerate([[1.,0.],[1.,0.],[0.,1.],[0.,1.]],1)]}
        return snapshot,checkpoint

    def test_deep_supported_topics_and_ancestors(self):
        s,c=self.inputs();r=frontier(s,c)
        self.assertEqual(r['selected'],['a','b'])
        self.assertEqual(r['threads'][0]['ancestor_tags'],['root'])
        self.assertEqual(r['summary']['coverage'],1.)

    def test_cross_topic_parent_rejected(self):
        s,c=self.inputs()
        for node in s['nodes'][:2]:node['labels']=[]
        # Root LOO cosine=.447: lower than the fixed .55 threshold.
        r=frontier(s,c)
        self.assertEqual(r['selected'],[])
        self.assertEqual(frontier(s,c,False)['selected'],['root'])

    def test_no_singleton_or_parent_coverage_fallback(self):
        s,c=self.inputs();s['nodes'][1]['labels']=[]
        r=frontier(s,c)
        self.assertEqual(r['selected'],['a'])
        self.assertEqual(r['summary']['coverage'],.5)
        self.assertEqual(r['memberships']['3'],[])

    def test_missing_geometry_rejected(self):
        s,c=self.inputs();c['records'][0]['vector']=None
        with self.assertRaises(ValueError):frontier(s,c)


if __name__=='__main__':unittest.main()
