from fractions import Fraction
import itertools
import random
from types import SimpleNamespace
from unittest import TestCase
from distinct_support import best_matching,evaluate,redundancy
from audit_distinct_support import exhaustive,verify_redundancy


class DistinctSupportTests(TestCase):
    def test_exact_optimizer_matches_exhaustive_oracle(self):
        rng=random.Random(718)
        for _ in range(150):
            height=rng.randrange(5);width=rng.randrange(5)
            weights=[[Fraction(rng.randrange(5),4) for _ in range(width)] for _ in range(height)]
            expected=[]
            for path in itertools.product(range(width+1),repeat=height):
                chosen=[col for col in path if col<width]
                if len(set(chosen))!=len(chosen):continue
                if any(col<width and not row[col] for row,col in zip(weights,path)):continue
                total=sum((row[col] for row,col in zip(weights,path) if col<width),Fraction(0))
                expected.append((total,len(chosen),path))
            total,count,path=min(expected,key=lambda row:(-row[0],-row[1],row[2]))
            self.assertEqual(best_matching(weights),(total,count,tuple(None if col==width else col for col in path)))

    def test_ties_abstention_bounds_and_invalid_scores(self):
        one=Fraction(1)
        self.assertEqual(best_matching([[one,one],[one,one]])[2],(0,1))
        self.assertEqual(best_matching([[Fraction(0)],[Fraction(0)]])[2],(None,None))
        for invalid in [[[1.]],[[Fraction(2)]],[[Fraction(-1)]],[[one],[]],[[one]*17]]:
            with self.assertRaises(ValueError):best_matching(invalid)

    def test_reuse_gap_identical_support_and_cross_thread_scope(self):
        snapshot={'topics':[{'id':'x','sources':['a','foreign']},{'id':'y','sources':['a','foreign']}],
            'threads':[{'thread':'t','sources':['a','b'],'tags':[{'topic':'y'},{'topic':'x'}]}]}
        subjects=[SimpleNamespace(thread='t',ordinal=0,importance='primary',sources={'a'}),
                  SimpleNamespace(thread='t',ordinal=1,importance='secondary',sources={'b'})]
        result=evaluate(snapshot,subjects)
        self.assertEqual(result['summary']['primary']['mean_distinct_support_f1'],1)
        self.assertEqual(result['summary']['secondary']['unmatched_subjects'],1)
        self.assertEqual(redundancy(snapshot)['summary']['identical_support_pairs'],1)
        subjects[1].sources={'a'}
        self.assertEqual(evaluate(snapshot,subjects)['total_reuse_gap'],0)
        snapshot['threads'][0]['tags'].pop()
        self.assertEqual(evaluate(snapshot,subjects)['total_reuse_gap'],1)

    def test_empty_unknown_duplicate_and_invalid_evidence(self):
        snapshot={'topics':[],'threads':[{'thread':'t','sources':['a'],'tags':[]}]}
        self.assertEqual(redundancy(snapshot)['threads'][0]['mean_pair_jaccard'],None)
        subject=SimpleNamespace(thread='t',ordinal=0,importance='primary',sources={'a'})
        self.assertEqual(evaluate(snapshot,[subject])['summary']['primary']['unmatched_subjects'],1)
        with self.assertRaises(ValueError):evaluate(snapshot,[subject,subject])
        subject.sources={'foreign'}
        with self.assertRaises(ValueError):evaluate(snapshot,[subject])
        snapshot['threads'][0]['tags']=[{'topic':'unknown'}]
        with self.assertRaises(ValueError):redundancy(snapshot)

    def test_auditor_oracle_and_corrupt_redundancy(self):
        one=Fraction(1)
        self.assertEqual(exhaustive([[one],[one]])[:2],(one,(0,None)))
        snapshot={'topics':[{'id':'x','sources':['a']}],
            'threads':[{'thread':'t','sources':['a'],'tags':[{'topic':'x'}]}]}
        observed=redundancy(snapshot)
        self.assertEqual(verify_redundancy(snapshot,observed),0)
        observed['threads'][0]['distinct_support_signatures']=0
        with self.assertRaises(AssertionError):verify_redundancy(snapshot,observed)
