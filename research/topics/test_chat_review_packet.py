import hashlib
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest
from chat_review_packet import Candidate, STRATA, answers, build, encoded, group, page, select


class ChatReviewTests(unittest.TestCase):
    def test_length_framing_and_order_preserve_original_sequences(self):
        self.assertNotEqual(group(['ab','c']),group(['a','bc']))
        self.assertNotEqual(group(['a','b']),group(['b','a']))
        self.assertNotEqual(group(['a','a']),group(['a']))

    def test_selection_is_order_independent_and_excludes_all_exact_aliases(self):
        candidates = [Candidate(f'{p}-{s}-{i}',p,s,hashlib.sha256(f'{p}-{s}-{i}'.encode()).hexdigest())
                      for p,s in STRATA for i in range(3)]
        alias = Candidate('alias','codex','short',candidates[0].group)
        excluded = {candidates[0].group}; left,support = select(candidates+[alias],excluded)
        self.assertEqual((left,support),select(list(reversed(candidates+[alias])),excluded))
        self.assertEqual(len(left),8); self.assertNotIn(candidates[0].group,{c.group for c in left})
        self.assertEqual(len({c.group for c in left}),8)
        _,small = select([],set()); self.assertTrue(all(v['selected'] == 0 for v in small.values()))

    def test_invalid_candidate_schema_and_counts_are_rejected(self):
        for count in [True,0,9]:
            with self.assertRaises(ValueError): select([],set(),count)
        for c in [Candidate('x','codex','short',''),Candidate('x','other','short','a'*64)]:
            with self.assertRaises(ValueError): select([c],set())

    def test_complete_indexed_context_repeat_and_explicit_redaction(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); database = root/'index.sqlite3'
            with sqlite3.connect(database) as connection:
                connection.executescript('CREATE TABLE conversations(id TEXT,provider TEXT); CREATE TABLE messages(id TEXT,conversation_id TEXT,ordinal INTEGER,role TEXT,content TEXT);')
                for p,s in STRATA:
                    thread = p+s; connection.execute('INSERT INTO conversations VALUES(?,?)',(thread,p))
                    count = 3 if s == 'short' else 11
                    for i in range(count): connection.execute('INSERT INTO messages VALUES(?,?,?,?,?)',(thread+str(i),thread,2*i,'user',thread+' question '+str(i)))
                    connection.execute('INSERT INTO messages VALUES(?,?,?,?,?)',(thread+'assistant',thread,1,'assistant','<script>data & text</script>'))
                    connection.execute('INSERT INTO messages VALUES(?,?,?,?,?)',(thread+'tool',thread,3,'tool','xY_7'*20))
                connection.execute('INSERT INTO conversations VALUES(?,?)',('excluded','codex'))
                connection.execute('INSERT INTO messages VALUES(?,?,?,?,?)',('e','excluded',0,'user','previously reviewed'))
            exclusion = root/'exclusions.toml'; exclusion.write_text('schema_version = 1\n[[cases]]\nthread = "excluded"\n')
            first,manifest = build(database,[exclusion],'rubric <img src=x>'); second,again = build(database,[exclusion],'rubric <img src=x>')
            for name in first:
                if name != 'provenance.json': self.assertEqual(first[name],second[name])
            manifest.pop('timings'); again.pop('timings'); self.assertEqual(manifest,again)
            self.assertEqual(manifest['selected_documents'],4); self.assertEqual(manifest['selected_turns'],36)
            self.assertEqual(manifest['display_redactions'],4); self.assertEqual(manifest['annotations_received'],0)
            packet = json.loads(first['review.json']); originals = json.loads(first['unblinded-originals.json'])
            for d in packet['documents']:
                self.assertEqual([t['ordinal'] for t in d['turns']],sorted(t['ordinal'] for t in d['turns']))
                for displayed,original in zip(d['turns'],originals[d['id']]['turns']):
                    self.assertEqual(displayed['original_text_sha256'],hashlib.sha256(original['text'].encode()).hexdigest())
                    if not displayed['display_redacted']: self.assertEqual(displayed['text'],original['text'])
            output = first['review.html'].decode(); self.assertNotIn('<script>',output); self.assertNotIn('<img',output)
            self.assertIn('&lt;script&gt;',output); self.assertIn('default-src',output)
            template = json.loads(first['answers.json']); self.assertTrue(all(v['subjects'] is None for v in template['documents']))
            self.assertEqual(template,answers(packet,hashlib.sha256(encoded(packet)).hexdigest()))


if __name__ == '__main__': unittest.main()
