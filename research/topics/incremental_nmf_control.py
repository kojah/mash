"""Frozen matched incremental NMF pilot; exposed support proxies, no promotion."""
import argparse
from importlib.metadata import version
import json
from pathlib import Path
import resource
import time
import sklearn.decomposition._nmf as author
from sklearn.metrics import adjusted_rand_score
from incremental_nmf import experiment
from phrase_review_evaluation import load,evaluate,digest
from topic_cohort_demo import load as cohort,PINS
from topic_phrases import PhraseSource
from distinct_support import evaluate as distinct, redundancy
from phrase_selection_control import referential_only


def hard(snapshot):
    topics={row['id']:row['component'] for row in snapshot['topics']}
    return [topics[min(tags,key=lambda tag:(-tag['score'],topics[tag['topic']]))['topic']] if tags else -1
        for _,tags in sorted(snapshot['memberships'].items())]


def run(args):
    names=sorted(set(PINS+['incremental_nmf.py','incremental_nmf_control.py','INCREMENTAL_NMF_PROTOCOL.md',
        'phrase_review_evaluation.py','distinct_support.py','topic_features.py','subject_labels.py','label_noise.py']))
    pins={name:digest(Path(__file__).with_name(name)) for name in names}
    hashes={name:digest(getattr(args,name)) for name in ['packet','review','database','selection','baseline']}
    old=json.loads(args.baseline.read_text())
    for name in ['packet','review','database','selection']:
        if old['input_hashes'][name]!=hashes[name]:raise ValueError('matched source drift')
    if version('scikit-learn')!='1.9.1':raise ValueError('pinned scientific sklearn required')
    started=time.perf_counter();cpu=time.process_time();sources,subjects,policy=load(args.packet,args.review)
    broad,_,_,_,ledger=cohort(args.database,args.selection);broad=[PhraseSource(row.id,row.thread,row.provider,row.text) for row in broad]
    packet=json.loads(args.packet.read_text());review=json.loads(args.review.read_text());outputs={};timings={};order_comparisons={}
    for scope,points in [('review',sources),('cohort',broad)]:
        outputs[scope]={};order_comparisons[scope]={}
        namespace=old['outputs'][scope]['source_unit_candidate_coverage']
        if scope=='review':namespace=namespace['snapshot']
        namespace=namespace['namespace']+'#nmf'
        for count in [16,32]:
            for passes in [1,5]:
                for reverse in [False,True]:
                    name=f'k{count}-passes{passes}-'+('reverse' if reverse else 'forward')
                    arm=experiment(points,count,passes,reverse,namespace);timings[scope+'_'+name]=arm.pop('runtime')
                    arm['redundancy']=redundancy(arm['snapshot'])
                    if scope=='review':
                        rows,summary=evaluate(arm['snapshot'],subjects);arm.update(subjects=rows,summary=summary,
                            distinct=distinct(arm['snapshot'],subjects),
                            referential_only_topics=referential_only(arm['snapshot'],packet,review))
                    outputs[scope][name]=arm
                    print(json.dumps({'progress':scope+'_'+name,'diagnostics':arm['snapshot']['diagnostics'],
                        'warnings':arm['warning_counts'],'runtime':timings[scope+'_'+name]}),flush=True)
                first=outputs[scope][f'k{count}-passes{passes}-forward']['snapshot']
                second=outputs[scope][f'k{count}-passes{passes}-reverse']['snapshot']
                order_comparisons[scope][f'k{count}-passes{passes}']={'dominant_membership_ari':float(adjusted_rand_score(hard(first),hard(second))),
                    'scope':'all original source IDs; unassigned one category; not multi-label or semantic agreement'}
    if hashes!={name:digest(getattr(args,name)) for name in hashes} or pins!={name:digest(Path(__file__).with_name(name)) for name in pins}:
        raise ValueError('input/code drift')
    return {'schema_version':1,'scope':'frozen_incremental_nmf_exposed_development_not_semantic_selection',
        'code_pins':pins,'input_hashes':hashes,'review_policy':policy,'source_ledger_sha256':ledger,
        'versions':{name:version(name) for name in ['scikit-learn','numpy','scipy']},'author_nmf_sha256':digest(Path(author.__file__)),
        'outputs':outputs,'order_comparisons':order_comparisons,'semantic_quality_verified':False,'production_selected':False,
        'runtime':{'arms':timings,'total_after_initial_hashes_ms':1000*(time.perf_counter()-started),
            'cpu_ms':1000*(time.process_time()-cpu),'rusage_before_serialization_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['packet','review','database','selection','baseline','output']:parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--compare',type=Path);args=parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh private external output required')
    result=run(args)
    if args.compare:
        old=json.loads(args.compare.read_text());current=dict(result);old.pop('runtime');current.pop('runtime')
        if old!=current:raise ValueError('non-runtime NMF pilot mismatch')
    with args.output.open('x') as file:json.dump(result,file,sort_keys=True,allow_nan=False)
    args.output.chmod(0o600)
    print(json.dumps({'complete':True,'order_comparisons':result['order_comparisons'],'runtime':result['runtime']}),flush=True)
