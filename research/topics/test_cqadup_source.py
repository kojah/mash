import dataclasses
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from cqadup_source import Reference, load, parse_references, parse_source, partition


class CQADupSourceTests(unittest.TestCase):
    def source(self, identity, text, query=False):
        value = {'_id': identity, 'text': text, 'metadata': {'tags': ['hidden'], 'prediction': 'hidden'}}
        if not query:value['title'] = ''
        return parse_source(value,query)

    def fixture(self):
        documents = {i:self.source(i,i) for i in ('a','b','c','d')}
        queries = {i:self.source(i,i,True) for i in ('one','two','three','four')}
        references = [Reference('one','a',1),Reference('two','a',1),Reference('three','b',1),Reference('four','c',1)]
        return documents,queries,references

    def test_source_fields_exclude_metadata_preserve_unicode(self):
        source=self.source('id','é🙂\r\ncode',True)
        self.assertEqual(set(dataclasses.asdict(source)),{'id','title','text','content_sha256'})
        self.assertEqual(source.text,'é🙂\r\ncode')
        self.assertEqual(source.title,'')
        self.assertNotIn('hidden',str(source))

    def test_strict_sources_and_references(self):
        for value in ({'_id':'x','text':'text','metadata':[]},
                      {'_id':1,'text':'text','metadata':{}},
                      {'_id':'x','text':'','metadata':{}},
                      {'_id':'x','text':'x'*65537,'metadata':{}},
                      {'_id':'x','text':'text','metadata':{},'extra':1}):
            with self.assertRaises(ValueError):parse_source(value,True)
        d,q,r=self.fixture()
        valid=b'query-id\tcorpus-id\tscore\none\ta\t1\n'
        self.assertEqual(parse_references(valid,q,d),(Reference('one','a',1),))
        for raw in (b'wrong\n',valid+b'one\ta\t1\n',valid+b'missing\ta\t1\n',valid+b'two\tb\t0\n'):
            with self.assertRaises(ValueError):parse_references(raw,q,d)

    def test_family_partition_disjoint_stable_and_no_relevance_propagation(self):
        d,q,r=self.fixture()
        first=partition(d,q,r,1,1)
        second=partition(dict(reversed(list(d.items()))),dict(reversed(list(q.items()))),list(reversed(r)),1,1)
        self.assertEqual(first,second)
        groups=[g for phase in first['phases'].values() for g in phase]
        same=next(g for g in groups if 'one' in g['queries'])
        self.assertIn('two',same['queries'])
        nodes=[n for g in groups for n in g['members']]
        self.assertEqual(len(nodes),len(set(nodes)))
        self.assertEqual(sum(len(g['queries']) for g in groups),4)
        self.assertEqual(len(r),4)

    def test_exact_content_merges_and_insufficient_families_fail(self):
        d,q,r=self.fixture()
        q['four']=self.source('four','three',True)
        output=partition(d,q,r,1,1)
        self.assertEqual(output['query_families'],2)
        with self.assertRaises(ValueError):partition(d,q,r,2,1)
        q['four']=dataclasses.replace(q['four'],content_sha256='invented')
        with self.assertRaises(ValueError):partition(d,q,r,1,1)
        for budget in (True,0,129):
            with self.assertRaises(ValueError):partition(d,q,r,budget,1)

    def test_complete_capture_hashes_and_unique_json_fields(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            values={'corpus.jsonl':json.dumps({'_id':'a','title':'title','text':'body','metadata':{}}).encode()+b'\n',
                    'queries.jsonl':json.dumps({'_id':'q','text':'query','metadata':{'tags':['hidden']}}).encode()+b'\n',
                    'qrels.tsv':b'query-id\tcorpus-id\tscore\nq\ta\t1\n'}
            manifest={'member_crc_verified':True,'files':{name:{'sha256':hashlib.sha256(raw).hexdigest(),'size':len(raw)}
                                                        for name,raw in values.items()}}
            raw=json.dumps(manifest).encode();pin=hashlib.sha256(raw).hexdigest()
            (root/'capture.json').write_bytes(raw)
            for name,value in values.items():(root/name).write_bytes(value)
            d,q,refs,summary=load(root,pin)
            self.assertEqual(q['q'].text,'query')
            self.assertEqual(len(refs),1)
            self.assertEqual(summary['queries.jsonl']['feature_text_bytes'],5)
            with self.assertRaises(ValueError):load(root,'changed')
            (root/'queries.jsonl').write_bytes(values['queries.jsonl'].replace(b'query',b'other'))
            with self.assertRaises(ValueError):load(root,pin)
            for malformed in (
                b'{"_id":"q","text":"query","text":"other","metadata":{}}\n',
                b'{"_id":"q","text":"query","metadata":{"value":NaN}}\n',
            ):
                # Pin the malformed bytes so rejection must come from decoding,
                # rather than from an unrelated content-hash mismatch.
                (root/'queries.jsonl').write_bytes(malformed)
                manifest['files']['queries.jsonl'] = {
                    'sha256': hashlib.sha256(malformed).hexdigest(),
                    'size': len(malformed),
                }
                raw = json.dumps(manifest).encode()
                (root/'capture.json').write_bytes(raw)
                with self.assertRaises(ValueError):
                    load(root, hashlib.sha256(raw).hexdigest())


if __name__=='__main__':
    unittest.main()
