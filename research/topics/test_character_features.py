import unittest

import numpy as np

from character_features import count_channels, idf_vectors, sparse_rows


class CharacterFeatureTests(unittest.TestCase):
    def test_identifiers_remain_distinct_without_name_specific_rules(self):
        title = ['C++', 'C++', 'C#', 'C#', 'python', 'ruby', 'rust', 'golang']
        terms, counts = count_channels(title, title, ['']*8, ['']*8)
        self.assertIn('c++', terms)
        self.assertIn(' c#', terms)
        self.assertFalse(np.array_equal(counts[0][0], counts[0][2]))
        self.assertTrue(np.array_equal(counts[0][0], counts[0][1]))
        self.assertFalse(counts[1].any() or counts[2].any())

    def test_source_conservation_and_whitespace_padding(self):
        title = ['αβ', 'αβ', 'C++', 'C++', 'one', 'two', 'three', 'four']
        prose = ['same text']*2 + ['different prose']*2 + ['fifth', 'sixth', 'seven', 'eight']
        code = ['func()', 'func()', '', '', '', '', '', '']
        full = [a+'\n'+b+'\r\n'+c for a,b,c in zip(title,prose,code)]
        terms, matrices = count_channels(full, title, prose, code)
        self.assertEqual(len(matrices), 3)
        self.assertTrue(all(m.shape == (8,len(terms)) for m in matrices))
        with self.assertRaises(ValueError):
            count_channels(['other']*8, title, prose, code)

    def test_idf_formula_norm_zero_and_sparse_roundtrip(self):
        mass = np.array([[.5,.5,0], [0,1,0], [0,0,0]])
        counts = [[1,1,0], [0,4,0], [0,0,0]]
        vectors, df, idf = idf_vectors(mass,counts)
        self.assertEqual(df.tolist(),[1,2,0])
        np.testing.assert_array_equal(idf,np.log(4/(1+df))+1)
        np.testing.assert_allclose(np.linalg.norm(vectors,axis=1),[1,1,0],atol=1e-12)
        rebuilt = np.zeros_like(vectors)
        for i,row in enumerate(sparse_rows(vectors)):
            rebuilt[i,row['indices']] = row['values']
        np.testing.assert_array_equal(vectors,rebuilt)

    def test_bad_sources_and_empty_vocabulary_rejected(self):
        for fields in [([],[],[],[]), (['x'],[],[''],['']), (['x'*65537],['x'],[''],['']),
                       (['x'],[None],[''],['']), ([''],[''],[''],[''])]:
            with self.assertRaises(ValueError):
                count_channels(*fields)

    def test_invalid_mass_counts_rejected(self):
        for mass,counts in [([[1]],[[1,2]]), ([[np.nan]],[[1]]), ([[1]],[[-1]]),
                            ([[np.inf]],[[1]]), ([],[]), ([[0]],[[np.inf]])]:
            with self.assertRaises(ValueError):
                idf_vectors(mass,counts)


if __name__ == '__main__':
    unittest.main()
