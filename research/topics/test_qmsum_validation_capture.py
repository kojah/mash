import hashlib
from dataclasses import replace
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import qmsum_validation_capture as validation


def blob(raw):
    return hashlib.sha1(b'blob '+str(len(raw)).encode()+b'\0'+raw).hexdigest()


class ValidationCapture(unittest.TestCase):
    def setUp(self):
        meeting = {'meeting_transcripts':[{'speaker':'A','content':'Résumé 😀 source.'}],
            'topic_list':[{'topic':'Original topic','relevant_text_span':[['0','0']]}],
            'general_query_list':[{'query':'Whole meeting?','answer':'Original answer.'}],
            'specific_query_list':[{'query':'Original query?','answer':'Original answer.','relevant_text_span':[['0','0']]}]}
        self.contents = {'README.md':b'Synthetic source fixture\n','LICENSE':b'Synthetic license fixture\n',
                         validation.SOURCE:json.dumps(meeting,ensure_ascii=False).encode()+b'\n'}
        self.pins = {path:(blob(raw),len(raw)) for path,raw in self.contents.items()}
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)/'capture'
        self.files = patch.dict(validation.FILES,self.pins,clear=True); self.files.start(); self.addCleanup(self.files.stop)
        def fetch(url,timeout):
            self.assertEqual(timeout,30)
            for path,raw in self.contents.items():
                if url == validation.url(path):return io.BytesIO(raw)
            self.fail('unexpected source request: '+url)
        with patch.object(validation.urllib.request,'urlopen',side_effect=fetch) as request:
            validation.capture(self.root)
            self.assertEqual(request.call_count,3)

    def mutate(self,field,value):
        p = self.root/'capture.json'; data = json.loads(p.read_text()); data[field] = value; p.write_text(json.dumps(data))

    def test_typed_original_source_and_span_identity_round_trip(self):
        source = validation.load(self.root)
        self.assertIsInstance(source,validation.ValidationSource)
        self.assertEqual(source.source_sha256,hashlib.sha256(self.contents[validation.SOURCE]).hexdigest())
        self.assertEqual(source.meetings[0].turns[0].text,'Résumé 😀 source.')
        span = source.meetings[0].specific_queries[0].spans[0]
        self.assertEqual((span.start,span.end,span.original_start,span.original_end),(0,0,'0','0'))
        self.assertFalse((self.root/'data/ALL/jsonl/test.jsonl').exists())

    def test_split_protocol_and_header_drift_rejected(self):
        original = (self.root/'capture.json').read_bytes()
        for field,value in [('scope','author-training-topic-span-source-preflight'),('protocol_sha256','0'*64),
                            ('test_downloaded',True),('inference_calls',True),('schema_version',True),('tree','0'*40)]:
            with self.subTest(field=field):
                (self.root/'capture.json').write_bytes(original);self.mutate(field,value)
                with self.assertRaises(ValueError):validation.load(self.root)

    def test_source_bytes_and_extra_test_file_rejected(self):
        path = self.root/validation.SOURCE; path.write_bytes(self.contents[validation.SOURCE]+b' ')
        with self.assertRaises(ValueError):validation.load(self.root)
        path.write_bytes(self.contents[validation.SOURCE]); (path.parent/'test.jsonl').write_bytes(b'{}\n')
        with self.assertRaises(ValueError):validation.load(self.root)

    def test_file_size_json_type_must_be_integer(self):
        path = self.root/'capture.json'; metadata = json.loads(path.read_text())
        metadata['files']['README.md']['bytes'] = float(metadata['files']['README.md']['bytes'])
        path.write_text(json.dumps(metadata))
        with self.assertRaises(ValueError):validation.load(self.root)

    def test_symlinks_and_nonfresh_destination_rejected(self):
        with self.assertRaises(ValueError):validation.capture(self.root)
        outside = Path(self.temp.name)/'outside.jsonl'; path = self.root/validation.SOURCE; path.rename(outside);path.symlink_to(outside)
        with self.assertRaises(ValueError):validation.load(self.root)

    def test_duplicate_features_rejected_even_with_changed_speaker_and_annotation(self):
        source = validation.load(self.root); meeting = source.meetings[0]
        duplicate = replace(meeting,turns=(replace(meeting.turns[0],speaker='Different alias'),),
                            specific_queries=(replace(meeting.specific_queries[0],query='Different query?'),))
        with self.assertRaises(ValueError):validation.check_disjoint(source,(duplicate,))
        with self.assertRaises(ValueError):validation.check_disjoint(replace(source,meetings=(meeting,meeting)),())

    def test_shared_turn_texts_counted_without_dropping_nonduplicate_meetings(self):
        source = validation.load(self.root); meeting = source.meetings[0]
        different = replace(meeting,turns=(meeting.turns[0],replace(meeting.turns[0],ordinal=1,text='Different original turn.')))
        result = validation.check_disjoint(source,(different,))
        self.assertEqual((result.training_meetings,result.validation_meetings),(1,1))
        self.assertEqual((result.shared_unique_turn_texts,result.shared_validation_turn_occurrences),(1,1))


if __name__ == '__main__':unittest.main()
