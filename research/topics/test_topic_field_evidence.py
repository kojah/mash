import unittest

import numpy as np

from cqadup_source import Source
from topic_field_evidence import Field,counts
from topic_labels import classes,labels
from topic_routing import Match


class FieldEvidenceTests(unittest.TestCase):
    def test_exact_field_boundaries_unicode_and_fixed_vocabulary(self):
        docs=[Source('a','🙂 Straße C++','C# foo.bar','hash'),Source('b','ignored','C++','hash')]
        terms=sorted(['strasse','c++','strasse c++','c#','foo.bar','c# foo.bar'])
        title=counts(docs,terms,Field.TITLE);body=counts(docs,terms,Field.BODY)
        self.assertEqual(title[0,terms.index('strasse c++')],1)
        self.assertEqual(body[0,terms.index('c# foo.bar')],1)
        self.assertEqual(title[1].nnz,0)
        self.assertEqual((title+body)[1,terms.index('c++')],1)
        with self.assertRaises(ValueError): counts(docs,terms,'title')

    def test_zero_title_evidence_keeps_topic_row_and_empty_labels(self):
        docs=[Source('a','outside','term','hash'),Source('b','term','body','hash')]
        title=counts(docs,['term'],Field.TITLE);background=title+counts(docs,['term'],Field.BODY)
        aggregated=classes(title,[(Match(0,.9),),(Match(1,.9),)],2)
        np.testing.assert_array_equal(aggregated.toarray(),[[0],[1]])
        result=labels(aggregated,background,['term'])
        self.assertTrue(all(not row for row in result[0].values()))
        self.assertEqual(result[1]['corpus-idf'][0]['term'],'term')


if __name__=='__main__': unittest.main()
