"""No producer imports: exact phrase rankings, scores, identity and evidence."""
from collections import Counter,defaultdict
import argparse
import hashlib
import json
import math
from pathlib import Path
from topic_capacity_audit import counts

PHATIC=set('yes no thanks thank please hello hi hey greetings bye sorry actually basically anyway anyways lol lmao alright okok kk uhh umm hmmm'.split())


def audit_snapshot(rows,forward):
    rows=sorted(rows,key=lambda row:row['id']);source={row['id']:row for row in rows};words={sid:counts(row['text']) for sid,row in source.items()}
    threads=defaultdict(list);inputs=defaultdict(set);support=defaultdict(set)
    for row in rows:
        threads[row['thread']].append(row)
        for term in words[row['id']]:inputs[term].add(row['text']);support[term].add(row['thread'])
    classes={};total=Counter()
    for thread,items in sorted(threads.items()):
        seen=set();pool=Counter()
        for row in items:
            if row['text'] not in seen:pool.update(words[row['id']]);seen.add(row['text'])
        classes[thread]=pool;total.update(pool)
    vocabulary=sorted(sorted(total,key=lambda term:(-total[term],term))[:8192]);keep=set(vocabulary)
    weighted={thread:{term:1+math.log(n) for term,n in pool.items() if term in keep} for thread,pool in classes.items()}
    frequency=Counter()
    for pool in weighted.values():frequency.update(pool)
    lengths={thread:sum(pool.values()) for thread,pool in weighted.items()};mean=sum(lengths.values())/len(lengths)
    checked=0;maximum_error=0.;expected_topics={};expected_memberships={sid:[] for sid in source}
    thread_map={row['thread']:row for row in forward['threads']}
    assert set(thread_map)==set(threads) and len(thread_map)==len(forward['threads'])
    for thread,items in sorted(threads.items()):
        ranked=[]
        for term in vocabulary:
            df=len(support[term])
            if not any(c.isalpha() for c in term) or (len(threads)>=3 and df>.4*len(threads)):continue
            tf=weighted[thread].get(term,0.)
            score=tf/max(lengths[thread],1)*math.log1p(mean/max(frequency[term],1))*(math.log((1+len(threads))/(1+df))+1)
            if score>0:ranked.append((score,term))
        selected=[];atoms=[]
        for score,term in sorted(ranked,key=lambda item:(-item[0],item[1])):
            parts=set(term.split())
            if any(parts<=prior or prior<=parts for prior in atoms):continue
            atoms.append(parts);selected.append((score,term))
            if len(selected)==8:break
        policy=forward['parameters'].get('selection')
        if policy in {'source_unit_candidate_coverage','phatic_filtered_candidate_coverage'}:
            if policy=='phatic_filtered_candidate_coverage':
                assert forward['parameters']['candidate_atom_exclusions']==sorted(PHATIC)
                ranked=[(score,term) for score,term in ranked if not set(term.split())&PHATIC]
            base={term:score for score,term in ranked};votes=defaultdict(set)
            for text in sorted({row['text'] for row in items}):
                present=counts(text);shortlist=[]
                for score,term in sorted(ranked,key=lambda item:(-item[0],item[1])):
                    if term not in present:continue
                    parts=set(term.split())
                    if any(parts<=set(other.split()) or set(other.split())<=parts for other in shortlist):continue
                    shortlist.append(term);votes[term].add(text)
                    if len(shortlist)==2:break
            selected=[];covered=set();available=set(votes);scale=max((base[t] for t in available),default=1.)
            while available and len(selected)<8:
                choices=[]
                for term in available:
                    parts=set(term.split())
                    if not any(parts<=set(other.split()) or set(other.split())<=parts for _,other in selected):choices.append(term)
                if not choices:break
                gains={term:len(votes[term]-covered) for term in choices};maximum=max(gains.values())
                def ordering(term):
                    utility=.5*base[term]/scale+(.5*gains[term]/maximum if maximum else 0.)
                    return (-utility,-base[term],term)
                term=sorted(choices,key=ordering)[0];selected.append((base[term],term))
                covered|=votes[term];available.remove(term)
        actual=thread_map[thread];assert len(actual['tags'])==len(selected)
        assert set(actual['sources'])=={row['id'] for row in items} and len(actual['sources'])==len(items)
        assert all(row['provider']==actual['provider'] for row in items)
        for (score,term),tag in zip(selected,actual['tags']):
            identity='phrase-'+hashlib.sha256((forward['namespace']+'\0'+term).encode()).hexdigest()
            assert tag['topic']==identity and math.isfinite(tag['score'])
            maximum_error=max(maximum_error,abs(score-tag['score']));assert math.isclose(score,tag['score'],rel_tol=1e-12,abs_tol=1e-12)
            status='cross_thread_and_distinct_input' if len(support[term])>=2 and len(inputs[term])>=2 else 'provisional'
            topic=expected_topics.setdefault(identity,{'phrase':term,'support':status,'threads':set(),'sources':set(),
                'corpus_thread_support':len(support[term]),'corpus_distinct_inputs':len(inputs[term])})
            topic['threads'].add(thread)
            for row in items:
                if term in words[row['id']]:
                    topic['sources'].add(row['id']);expected_memberships[row['id']].append({'topic':identity,'score':score})
            checked+=1
    actual_topics={topic['id']:topic for topic in forward['topics']}
    assert set(actual_topics)==set(expected_topics) and len(actual_topics)==len(forward['topics'])
    for identity,expected in expected_topics.items():
        actual=actual_topics[identity]
        for key,value in expected.items():assert (set(actual[key]) if isinstance(value,set) else actual[key])==value
        assert len(actual['sources'])==len(expected['sources']) and len(actual['threads'])==len(expected['threads'])
    assert set(forward['memberships'])==set(source)==set(forward['versions'])
    for sid,tags in expected_memberships.items():
        actual=forward['memberships'][sid];assert len(tags)==len(actual)
        for left,right in zip(tags,actual):
            assert left['topic']==right['topic'] and math.isclose(left['score'],right['score'],rel_tol=1e-12,abs_tol=1e-12)
    return {'sources_checked':len(source),'threads_checked':len(threads),'topics_checked':len(actual_topics),'thread_ranks_and_scores_checked':checked,'maximum_score_error':maximum_error}


def audit(source_baseline,first,repeat):
    rows=json.loads(source_baseline.read_text())['baseline_checkpoint']['records']
    a=json.loads(first.read_text());b=json.loads(repeat.read_text());a.pop('runtime');b.pop('runtime');assert a==b
    checked=audit_snapshot(rows,a['forward'])
    left=dict(a['forward']);right=dict(a['reverse'])
    for snapshot in [left,right]:snapshot.pop('revision');snapshot.pop('versions')
    assert left==right
    return dict(checked,stable_repeat=True,reverse_semantics_exact=True,
        shared_dependency='sklearn static English stoplist through independent scanner',
        scope='original source text, independent counts/duplicate handling/vocabulary/full ranking/score/identity/thread and source unions',
        semantic_quality_verified=False,lifecycle_independently_replayed=False)


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['source_baseline','first','repeat']:parser.add_argument('--'+name.replace('_','-'),type=Path,required=True)
    args=parser.parse_args();print(json.dumps(audit(args.source_baseline,args.first,args.repeat),sort_keys=True))
