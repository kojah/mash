import hashlib
import unittest

from tokenizers import Tokenizer, models, normalizers, pre_tokenizers, processors

from embedding_windows import Field, unbounded_tokenizer, windows


def tokenizer():
    t = Tokenizer(models.WordPiece({'[UNK]': 0, '[CLS]': 1, '[SEP]': 2, 'hello': 3, 'a': 4, '##b': 5}))
    t.normalizer = normalizers.BertNormalizer(lowercase=True)
    t.pre_tokenizer = pre_tokenizers.BertPreTokenizer()
    t.post_processor = processors.TemplateProcessing(single='[CLS] $A [SEP]', special_tokens=[('[CLS]', 1), ('[SEP]', 2)])
    return t


class EmbeddingWindowTests(unittest.TestCase):
    def test_complete_original_unicode_bytes_and_wordpiece_boundaries(self):
        text = '  Hello ab é e\u0301 🙂\r\n' * 180
        t = tokenizer()
        result = windows(t, 'source', Field.BODY, text)
        self.assertGreater(len(result), 1)
        self.assertEqual(''.join(text[w.start:w.end] for w in result), text)
        self.assertEqual(result[-1].byte_end, len(text.encode()))
        for i, w in enumerate(result):
            part = text[w.start:w.end]
            self.assertEqual(w.ordinal, i)
            self.assertEqual(w.byte_start, len(text[:w.start].encode()))
            self.assertEqual(text.encode()[w.byte_start:w.byte_end].decode(), part)
            self.assertEqual(w.text_sha256, hashlib.sha256(part.encode()).hexdigest())
            self.assertEqual(w.ids, tuple(t.encode(part).ids))
            self.assertLessEqual(len(w.ids), 256)
            self.assertEqual(w.payload_tokens, len(t.encode(part, add_special_tokens=False).ids))

    def test_whole_word_cut_prevents_continuation_retokenization(self):
        t = tokenizer()
        text = ('ab ' * 130).rstrip()
        result = windows(t, 'source', Field.TITLE, text)
        self.assertEqual(len(result), 2)
        self.assertEqual(result[0].payload_tokens, 254)
        self.assertEqual(sum(w.payload_tokens for w in result), 260)
        self.assertEqual(result[0].ids[1:3], (4, 5))
        self.assertEqual(result[1].ids[1:3], (4, 5))

    def test_exact_boundary_short_and_empty(self):
        t = tokenizer()
        self.assertEqual(len(windows(t, 's', Field.BODY, 'hello '*254)), 1)
        self.assertEqual(len(windows(t, 's', Field.BODY, 'hello '*255)), 2)
        self.assertEqual(windows(t, 's', Field.BODY, '\t\r\n'), ())
        t.enable_truncation(4)
        self.assertEqual(len(windows(t, 's', Field.BODY, 'hello '*10)[0].ids), 12)

    def test_invalid_field_identity_and_budget_fail(self):
        t = tokenizer()
        for identity, field, text in [('', Field.BODY, 'hello'), ('s','body','hello'), ('s',Field.BODY,'x'*65537)]:
            with self.assertRaises(ValueError): windows(t, identity, field, text)

    def test_persisted_padding_and_truncation_disabled_before_first_measurement(self):
        t = tokenizer()
        t.enable_truncation(4)
        t.enable_padding(length=128, pad_id=0, pad_token='[UNK]')
        restored = unbounded_tokenizer(t.to_str())
        self.assertEqual(len(restored.encode('hello '*10).ids), 12)
        self.assertEqual(len(restored.encode('hello '*300).ids), 302)


if __name__ == '__main__':
    unittest.main()
