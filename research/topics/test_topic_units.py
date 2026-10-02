import unittest
import numpy as np
from topic_stream import Source
from topic_units import units,Adjacency


def source(identity,vector,text='fixture',thread='t'):
    return Source(identity,thread,'p',text,None if vector is None else np.array(vector,dtype=float))


class UnitTests(unittest.TestCase):
    def test_admitted_adjacency_preserves_exclusions_without_forcing_boundary(self):
        rows=[source('a',[1,0]),source('excluded',None,''),source('b',[1,0])]
        groups=units(rows,adjacency=Adjacency.ADMITTED)
        self.assertEqual([[s.id for s in g] for g in groups],[['excluded'],['a','b']])
        self.assertEqual({s.id for g in groups for s in g},{s.id for s in rows})
        self.assertEqual([[s.id for s in g] for g in units(rows)],[['a'],['excluded'],['b']])

    def test_unknown_missing_geometry_still_breaks_admitted_adjacency(self):
        rows=[source('a',[1,0]),source('missing',None,'real unembedded request'),source('b',[1,0])]
        self.assertEqual(list(map(len,units(rows,adjacency=Adjacency.ADMITTED))),[1,1,1])

    def test_policy_and_provider_scope_are_strict(self):
        with self.assertRaises(ValueError):units([],adjacency='admitted')
        rows=[source('excluded',None,''),Source('other','t','different','fixture',np.array([1.,0.]))]
        with self.assertRaises(ValueError):units(rows)

    def test_adjacent_related_turns_pool_but_topic_change_does_not(self):
        rows=[source('a',[1,0]),source('b',[1,0]),source('c',[0,1])]
        self.assertEqual([[s.id for s in g] for g in units(rows)],[['a','b'],['c']])

    def test_missing_embedding_breaks_unit_and_remains_present(self):
        rows=[source('a',[1,0]),source('gap',None),source('b',[1,0])]
        self.assertEqual([[s.id for s in g] for g in units(rows)],[['a'],['gap'],['b']])

    def test_never_pools_across_threads(self):
        rows=[source('a',[1,0],thread='a'),source('b',[1,0],thread='b')]
        self.assertEqual(len(units(rows)),2)

    def test_turn_byte_bounds_and_oversized_singleton_preserve_sources(self):
        rows=[source(str(i),[1,0],'abc') for i in range(10)]
        self.assertEqual(list(map(len,units(rows))),[8,2])
        self.assertEqual(list(map(len,units(rows,maximum_joined_bytes=7))),[1]*10)
        huge=source('huge',[1,0],'x'*9000)
        self.assertEqual([[s.id for s in g] for g in units([huge,rows[0]])],[['huge'],['0']])

    def test_invalid_limits_and_empty_input(self):
        self.assertEqual(units([]),[])
        with self.assertRaises(ValueError):units([],maximum_turns=True)
        with self.assertRaises(ValueError):units([],maximum_joined_bytes=0)


if __name__=='__main__':unittest.main()
