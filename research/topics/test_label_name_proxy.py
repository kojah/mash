import unittest
from label_name_proxy import evaluate


class Names(unittest.TestCase):
    def test_exact_tokens_unknown_and_empty_names_accounted(self):
        result=evaluate([[0],[1]],[[{'term':'pool'}],[{'term':'index'}]],[0,0,1,1],[0,None,1,2],['database pool','search index','the and'])
        self.assertEqual(result,{'named_sources':2,'empty_name_sources':1,'unknown_or_mixed_sources':1,'hits':2,'rate':1})
        empty=evaluate([[0]],[[]],[0],[0],['database'])
        self.assertEqual(empty['named_sources'],1);self.assertEqual(empty['rate'],0)

    def test_incomplete_ownership_rejected(self):
        with self.assertRaises(ValueError):evaluate([[0]],[[{'term':'index'}]],[0,1],[0,0],['index'])


if __name__=='__main__':unittest.main()
