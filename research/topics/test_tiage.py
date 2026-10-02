import hashlib
import json
import unittest

from tiage import Shift,parse
from tiage_capture import verify


class TiageTests(unittest.TestCase):
    def test_original_unicode_whitespace_order_and_response_label_alignment(self):
        rows=[['  🙂 hello\r\n','-1'],['new subject','1'],['continuation','0']]
        raw=json.dumps({'2':rows,'1':[['first','-1'],['next','0']]}).encode()
        ds=parse(raw);self.assertEqual([d.id for d in ds],['2','1']);self.assertEqual(ds[0].turns[0].text,rows[0][0])
        self.assertEqual([t.ordinal for t in ds[0].turns],[0,1,2])
        self.assertEqual(ds[0].turns[1].shift,Shift.CHANGE)
        self.assertEqual(ds[0].source_sha256,hashlib.sha256(json.dumps(rows,ensure_ascii=False,separators=(',',':')).encode()).hexdigest())

    def test_root_sentinel_is_not_negative_or_an_internal_boundary(self):
        for rows in [[['a','0'],['b','1']],[['a','-1'],['b','-1']]]:
            with self.assertRaises(ValueError):parse(json.dumps({'1':rows}).encode())

    def test_unknown_label_empty_and_malformed_turns_rejected(self):
        for row in [['b','2'],['b',True],['b',0],[' \n','0'],['b','0','extra'],{'text':'b','label':'0'}]:
            with self.assertRaises(ValueError):parse(json.dumps({'1':[['a','-1'],row]}).encode())

    def test_empty_singleton_and_invalid_identities_rejected(self):
        for data in [{},[],{'1':[]},{'1':[['a','-1']]},{'01':[['a','-1'],['b','0']]},{'0':[['a','-1'],['b','0']]}]:
            with self.assertRaises(ValueError):parse(json.dumps(data).encode())

    def test_duplicate_dialogue_keys_fail_instead_of_overwrite(self):
        with self.assertRaises(ValueError):parse(b'{"1":[],"1":[]}')

    def test_text_edits_and_annotation_changes_alter_source_identity_hash(self):
        rows=[['a','-1'],['b','0']];old=parse(json.dumps({'1':rows}).encode())[0].source_sha256
        rows[1][1]='1';changed=parse(json.dumps({'1':rows}).encode())[0].source_sha256
        self.assertNotEqual(old,changed);rows[1][0]='b '
        self.assertNotEqual(changed,parse(json.dumps({'1':rows}).encode())[0].source_sha256)

    def test_author_blob_and_size_both_required(self):
        raw=b'hello\n';pin=verify(raw,'ce013625030ba8dba906f756967f9e9ca394464a',6)
        self.assertEqual(pin,hashlib.sha256(raw).hexdigest())
        for blob,size in [('0'*40,6),('ce013625030ba8dba906f756967f9e9ca394464a',5)]:
            with self.assertRaises(ValueError):verify(raw,blob,size)


if __name__=='__main__':unittest.main()
