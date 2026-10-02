"""Original-source, exhaustive joint-matching audit without producer imports."""
import argparse
from fractions import Fraction
import hashlib
import itertools
import json
from pathlib import Path


def exhaustive(weights):
    width=len(weights[0]) if weights else 0;best=None;checked=0
    choices=[tuple(i for i,value in enumerate(row) if value)+(width,) for row in weights]
    for path in itertools.product(*choices):
        columns=[col for col in path if col<width]
        if len(set(columns))!=len(columns):continue
        total=sum((row[col] for row,col in zip(weights,path) if col<width),Fraction(0))
        candidate=(-total,-len(columns),path);checked+=1
        if best is None or candidate<best:best=candidate
    return -best[0],tuple(None if col==width else col for col in best[2]),checked


def verify_redundancy(snapshot,observed):
    topics={row['id']:set(row['sources']) for row in snapshot['topics']};checks=0
    for thread,expected in zip(sorted(snapshot['threads'],key=lambda row:row['thread']),observed['threads']):
        assert thread['thread']==expected['thread']
        evidence=[frozenset(topics[tag['topic']]&set(thread['sources'])) for tag in thread['tags']]
        pairs=list(itertools.combinations(evidence,2))
        average=sum((Fraction(len(a&b),len(a|b)) for a,b in pairs),Fraction(0))/len(pairs) if pairs else None
        assert expected=={'thread':thread['thread'],'tags':len(evidence),'distinct_support_signatures':len(set(evidence)),
            'pairs':len(pairs),'identical_support_pairs':sum(a==b for a,b in pairs),
            'mean_pair_jaccard':float(average) if average is not None else None}
        checks+=len(pairs)
    assert len(snapshot['threads'])==len(observed['threads'])
    rows=observed['threads']
    assert observed['summary']=={'threads':len(rows),'tags':sum(row['tags'] for row in rows),
        'distinct_local_support_signatures':sum(row['distinct_support_signatures'] for row in rows),
        'pairs':sum(row['pairs'] for row in rows),'identical_support_pairs':sum(row['identical_support_pairs'] for row in rows),
        'threads_with_identical_support_tags':sum(row['distinct_support_signatures']<row['tags'] for row in rows)}
    return checks


def audit(args):
    first=json.loads(args.first.read_text());repeat=json.loads(args.repeat.read_text())
    first.pop('runtime');repeat.pop('runtime');assert first==repeat
    for name in ['capture','packet','review']:
        with getattr(args,name).open('rb') as file:assert hashlib.file_digest(file,'sha256').hexdigest()==first['input_hashes'][name]
    packet=json.loads(args.packet.read_text());review=json.loads(args.review.read_text());capture=json.loads(args.capture.read_text())
    assert first['input_hashes']['packet']==review['packet_sha256']
    cases={row['thread']:row for row in review['cases']};subjects={};admitted={}
    for case in packet['cases']:
        entry=cases[case['thread']];identities={}
        for turn,judgment in zip(case['turns'],entry['admission_judgments']):
            if judgment['kind'] not in {'content','referential'}:continue
            assert not turn['display_redacted']
            assert hashlib.sha256(turn['text'].encode()).hexdigest()==turn['text_sha256']==judgment['text_sha256']
            assert (turn['id'],turn['ordinal'])==(judgment['message'],judgment['ordinal'])
            identities[turn['ordinal']]=json.dumps([case['thread'],turn['id']],ensure_ascii=False,separators=(',',':'))
        admitted[case['thread']]=set(identities.values());subjects[case['thread']]=[
            (i,subject['importance'],set(identities[ordinal] for ordinal in subject['support_ordinals']))
            for i,subject in enumerate(entry['subjects']) if subject['importance']!='uncertain']
    assignments=0;pairs=0;subject_checks=0;maximum_summary_error=0.
    for mode,result in first['outputs'].items():
        snapshot=capture['outputs']['review'][mode]['snapshot'];topics={row['id']:set(row['sources']) for row in snapshot['topics']}
        observed={(row['thread'],row['subject_ordinal']):row for row in result['review']['subjects']};thread_checks={}
        for thread in snapshot['threads']:
            identity=thread['thread'];assert set(thread['sources'])==admitted[identity]
            tags=sorted(tag['topic'] for tag in thread['tags']);group=subjects[identity]
            support=[topics[tag]&set(thread['sources']) for tag in tags]
            weights=[[Fraction(2*len(expected&found),len(expected)+len(found)) for found in support] for _,_,expected in group]
            total,path,checked=exhaustive(weights);assignments+=checked
            independent=sum((max(row,default=Fraction(0)) for row in weights),Fraction(0))
            thread_checks[identity]={'thread':identity,'subjects':len(group),'tags':len(tags),
                'independent_best_f1_sum':float(independent),'distinct_best_f1_sum':float(total),
                'optimistic_reuse_gap':float(independent-total),'distinct_positive_pairs':sum(col is not None for col in path)}
            for (ordinal,importance,_),row,col in zip(group,weights,path):
                value=row[col] if col is not None else Fraction(0)
                assert observed[identity,ordinal]=={'thread':identity,'subject_ordinal':ordinal,'importance':importance,
                    'topic':tags[col] if col is not None else None,'f1_numerator':value.numerator,'f1_denominator':value.denominator,
                    'distinct_support_f1':float(value),'independent_best_support_f1':float(max(row,default=Fraction(0)))}
                subject_checks+=1
        assert result['review']['threads']==[thread_checks[key] for key in sorted(thread_checks)]
        assert len(observed)==sum(len(group) for group in subjects.values())
        assert result['review']['threads_with_reuse_gap']==sum(row['optimistic_reuse_gap']>0 for row in thread_checks.values())
        assert result['review']['total_reuse_gap']==sum(row['optimistic_reuse_gap'] for row in result['review']['threads'])
        for importance,summary in result['review']['summary'].items():
            rows=[row for row in observed.values() if row['importance']==importance]
            assert summary['subjects']==len(rows)
            assert summary['unmatched_subjects']==sum(row['topic'] is None for row in rows)
            for field in ['distinct_support_f1','independent_best_support_f1']:
                expected=sum((Fraction(row[field]) for row in rows),Fraction(0))/len(rows)
                error=abs(summary['mean_'+field]-float(expected));maximum_summary_error=max(maximum_summary_error,error)
                assert error<=1e-15  # Display means are floats; assignments/F1 rationals above are exact.
        pairs+=verify_redundancy(snapshot,result['review_redundancy'])
        pairs+=verify_redundancy(capture['outputs']['cohort'][mode],result['cohort_redundancy'])
    return {'stable_repeat':True,'subject_checks':subject_checks,'feasible_assignments_exhaustively_checked':assignments,
        'local_support_pairs_checked':pairs,'maximum_display_summary_error':maximum_summary_error,'semantic_quality_verified':False,
        'scope':'original_review_support_and_capture_evidence; no producer imports; not independent semantic annotation'}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['first','repeat','capture','packet','review']:parser.add_argument('--'+name,type=Path,required=True)
    print(json.dumps(audit(parser.parse_args()),sort_keys=True))
