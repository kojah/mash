import unittest

import numpy as np
from scipy import sparse

from cqadup_source import Source
from topic_labels import analyzer, classes, scores, source_terms, support
from topic_routing import Match


class TopicLabelTests(unittest.TestCase):
    def test_technical_terms_and_only_source_adjacent_phrases(self):
        terms=analyzer('C++ and C#; foo.bar()\nStraße\0other field')
        self.assertIn('c++',terms);self.assertIn('c#',terms);self.assertIn('foo.bar',terms)
        self.assertIn('strasse',terms);self.assertNotIn('strasse other',terms)
        self.assertNotIn('c# foo.bar',terms)
        self.assertIn('other field',terms)
        self.assertEqual(analyzer('x'*129),[])

    def test_unicode_original_offsets_and_evidence(self):
        doc=Source('id','🙂 Straße\nC++','body','hash')
        result=support(doc,'strasse c++')
        self.assertEqual(result['text'],'Straße\nC++')
        self.assertEqual(doc.title[result['start']:result['end']],result['text'])
        self.assertEqual(doc.title.encode()[result['byte_start']:result['byte_end']].decode(),result['text'])
        with self.assertRaises(ValueError): support(doc,'invented')

    def test_overlap_mass_conservation_and_independent_sparse_formula(self):
        counts=sparse.csr_matrix([[3.,1.],[1.,4.]])
        members=[(Match(0,.9),Match(1,.8)),(Match(1,.9),)]
        c=classes(counts,members,2)
        np.testing.assert_allclose(c.toarray(),[[1.5,.5],[2.5,4.5]])
        np.testing.assert_allclose(np.asarray(c.sum(axis=0)),np.asarray(counts.sum(axis=0)))
        a,b=scores(c,counts)
        dense=c.toarray();lengths=dense.sum(axis=1)
        expected=dense/lengths[:,None]*np.log1p(lengths.mean()/dense.sum(axis=0))
        np.testing.assert_allclose(a.toarray(),expected,atol=1e-12)
        np.testing.assert_allclose(b.toarray(),expected,atol=1e-12)


if __name__ == '__main__': unittest.main()
