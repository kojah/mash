"""Diagnostic-only exact joint subject/tag support matching; no discovery."""
import argparse
from fractions import Fraction
import hashlib
import json
from pathlib import Path
import time


def best_matching(weights):
    """Maximize total rational F1, then positive pairs, then lexical path.

    Rows are subjects ordered by original ordinal; columns are sorted topic IDs.
    Unmatched sorts after every column. Zero-weight edges remain unmatched.
    The bounded subset DP is exponential only in tags, not source count.
    """
    width=len(weights[0]) if weights else 0
    if width>16 or any(len(row)!=width for row in weights):
        raise ValueError('rectangular matrix with at most 16 tags required')
    if any(not isinstance(value,Fraction) or not 0<=value<=1 for row in weights for value in row):
        raise ValueError('exact F1 fractions required')
    states={0:(Fraction(0),0,())}
    def key(value):return (-value[0],-value[1],value[2])
    for row in weights:
        updated={}
        for mask,(score,count,path) in states.items():
            candidates=[(mask,(score,count,path+(width,)))]
            candidates.extend((mask|(1<<col),(score+value,count+1,path+(col,)))
                for col,value in enumerate(row) if value and not mask&(1<<col))
            for target,value in candidates:
                if target not in updated or key(value)<key(updated[target]):updated[target]=value
        states=updated
    total,count,path=min(states.values(),key=key)
    return total,count,tuple(None if col==width else col for col in path)


def local_support(snapshot):
    topics={topic['id']:set(topic['sources']) for topic in snapshot['topics']}
    if len(topics)!=len(snapshot['topics']):raise ValueError('duplicate topic identity')
    result={}
    for thread in snapshot['threads']:
        identity=thread['thread'];local=set(thread['sources']);tags=[tag['topic'] for tag in thread['tags']]
        if identity in result or len(set(tags))!=len(tags):raise ValueError('duplicate thread or tag')
        if any(tag not in topics for tag in tags):raise ValueError('unknown topic')
        result[identity]={tag:topics[tag]&local for tag in sorted(tags)}
        if any(not support for support in result[identity].values()):raise ValueError('tag without local evidence')
    return result


def evaluate(snapshot,subjects):
    local=local_support(snapshot);grouped={};rows=[];threads=[]
    for subject in subjects:
        if subject.thread not in local:raise ValueError('unknown reviewed thread')
        grouped.setdefault(subject.thread,[]).append(subject)
    for identity,group in sorted(grouped.items()):
        group=sorted(group,key=lambda subject:subject.ordinal)
        if len({subject.ordinal for subject in group})!=len(group):raise ValueError('duplicate reviewed ordinal')
        source_ids=set(next(row['sources'] for row in snapshot['threads'] if row['thread']==identity))
        if any(not subject.sources or not set(subject.sources)<=source_ids for subject in group):
            raise ValueError('review support outside thread')
        tags=list(local[identity]);weights=[]
        for subject in group:
            weights.append([Fraction(2*len(set(subject.sources)&local[identity][tag]),
                len(subject.sources)+len(local[identity][tag])) for tag in tags])
        total,count,path=best_matching(weights)
        independent=sum((max(row,default=Fraction(0)) for row in weights),Fraction(0))
        threads.append({'thread':identity,'subjects':len(group),'tags':len(tags),
            'independent_best_f1_sum':float(independent),'distinct_best_f1_sum':float(total),
            'optimistic_reuse_gap':float(independent-total),'distinct_positive_pairs':count})
        for subject,weight,col in zip(group,weights,path):
            value=weight[col] if col is not None else Fraction(0)
            rows.append({'thread':identity,'subject_ordinal':subject.ordinal,'importance':str(subject.importance),
                'topic':tags[col] if col is not None else None,'f1_numerator':value.numerator,
                'f1_denominator':value.denominator,'distinct_support_f1':float(value),
                'independent_best_support_f1':float(max(weight,default=Fraction(0)))})
    summary={}
    for importance in sorted({row['importance'] for row in rows}):
        group=[row for row in rows if row['importance']==importance]
        summary[importance]={'subjects':len(group),
            'mean_distinct_support_f1':sum(row['distinct_support_f1'] for row in group)/len(group),
            'mean_independent_best_support_f1':sum(row['independent_best_support_f1'] for row in group)/len(group),
            'unmatched_subjects':sum(row['topic'] is None for row in group)}
    return {'subjects':rows,'threads':threads,'summary':summary,
        'threads_with_reuse_gap':sum(row['optimistic_reuse_gap']>0 for row in threads),
        'total_reuse_gap':sum(row['optimistic_reuse_gap'] for row in threads)}


def redundancy(snapshot):
    rows=[]
    for identity,tags in sorted(local_support(snapshot).items()):
        values=list(tags.values());pairs=[]
        for i,first in enumerate(values):
            for second in values[i+1:]:pairs.append(Fraction(len(first&second),len(first|second)))
        rows.append({'thread':identity,'tags':len(values),'distinct_support_signatures':len({frozenset(v) for v in values}),
            'pairs':len(pairs),'identical_support_pairs':sum(value==1 for value in pairs),
            'mean_pair_jaccard':float(sum(pairs,Fraction(0))/len(pairs)) if pairs else None})
    return {'threads':rows,'summary':{'threads':len(rows),'tags':sum(row['tags'] for row in rows),
        'distinct_local_support_signatures':sum(row['distinct_support_signatures'] for row in rows),
        'pairs':sum(row['pairs'] for row in rows),
        'identical_support_pairs':sum(row['identical_support_pairs'] for row in rows),
        'threads_with_identical_support_tags':sum(row['distinct_support_signatures']<row['tags'] for row in rows)}}


def digest(path):
    with path.open('rb') as file:return hashlib.file_digest(file,'sha256').hexdigest()


def run(capture,packet,review):
    from phrase_review_evaluation import load
    pins={name:digest(Path(__file__).with_name(name)) for name in ['distinct_support.py','phrase_review_evaluation.py']}
    hashes={name:digest(path) for name,path in [('capture',capture),('packet',packet),('review',review)]}
    data=json.loads(capture.read_text())
    if any(data['input_hashes'][name]!=hashes[name] for name in ['packet','review']):raise ValueError('review input drift')
    _,subjects,policy=load(packet,review);start=time.perf_counter();outputs={}
    for mode,arm in sorted(data['outputs']['review'].items()):
        evaluation=evaluate(arm['snapshot'],subjects)
        for importance,summary in evaluation['summary'].items():
            old=arm['summary'][importance]['mean_best_single_phrase_support_f1']
            if abs(old-summary['mean_independent_best_support_f1'])>1e-12:raise ValueError('previous metric drift')
        outputs[mode]={'review':evaluation,'review_redundancy':redundancy(arm['snapshot']),
            'cohort_redundancy':redundancy(data['outputs']['cohort'][mode])}
    if hashes!={name:digest(path) for name,path in [('capture',capture),('packet',packet),('review',review)]}:
        raise ValueError('input changed during evaluation')
    if pins!={name:digest(Path(__file__).with_name(name)) for name in pins}:raise ValueError('code changed')
    return {'schema_version':1,'scope':'exposed_review_distinct_literal_support_diagnostic_not_semantic_quality',
        'input_hashes':hashes,'code_pins':pins,'review_policy':policy,'outputs':outputs,
        'matching_contract':'sum_exact_F1_then_positive_pair_count_then_ordinal_sorted_topic_path_unmatched_last',
        'semantic_quality_verified':False,'production_selected':False,
        'runtime':{'evaluation_ms':1000*(time.perf_counter()-start)}}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['capture','packet','review','output']:parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--compare',type=Path);args=parser.parse_args()
    result=run(args.capture,args.packet,args.review)
    if args.compare:
        old=json.loads(args.compare.read_text());current=dict(result);old.pop('runtime');current.pop('runtime')
        if old!=current:raise ValueError('non-runtime replay mismatch')
    with args.output.open('x') as file:json.dump(result,file,sort_keys=True,allow_nan=False)
    args.output.chmod(0o600)
    print(json.dumps({'summary':{mode:{'review':arm['review']['summary'],
        'reuse_gap':arm['review']['total_reuse_gap'],'threads_with_gap':arm['review']['threads_with_reuse_gap'],
        'cohort_redundancy':arm['cohort_redundancy']['summary']} for mode,arm in result['outputs'].items()},
        'runtime':result['runtime']},sort_keys=True))
