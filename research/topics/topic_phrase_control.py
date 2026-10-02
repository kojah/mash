"""Frozen phrase-subject pilot with duplicate, lifecycle and reverse controls."""
import argparse
import json
from pathlib import Path
import resource
import time
from topic_cohort_demo import load,digest,PINS
from topic_phrases import PhraseSource
from topic_phrase_store import PhraseStore


def stable(result):
    value=dict(result);value.pop('runtime')
    return json.dumps(value,sort_keys=True,allow_nan=False,separators=(',',':'))


def semantic(snapshot):
    value=dict(snapshot);value.pop('revision');value.pop('versions');return value


def duplicate_semantic(snapshot,original_ids):
    return {'topics':[{key:value for key,value in topic.items() if key!='sources'} for topic in snapshot['topics']],
        'threads':[{key:value for key,value in thread.items() if key!='sources'} for thread in snapshot['threads']],
        'memberships':{identity:tags for identity,tags in snapshot['memberships'].items() if identity in original_ids}}


def experiment(database,selection,baseline):
    names=PINS+['topic_phrases.py','topic_phrase_store.py','phrase_selection.py','topic_phrase_control.py']
    pins={name:digest(Path(__file__).with_name(name)) for name in names};baseline_sha=digest(baseline)
    sources,_,_,_,ledger=load(database,selection);old=json.loads(baseline.read_text())['arms']['flat_final']['forward']
    sources=[PhraseSource(s.id,s.thread,s.provider,s.text) for s in sources];start=time.perf_counter();cpu=time.process_time()
    model=PhraseStore('research_phrase_subjects_v1');begin=time.perf_counter()
    for source in sources:model.upsert(source)
    ingestion=(time.perf_counter()-begin)*1000;begin=time.perf_counter();initial=model.snapshot();refresh=(time.perf_counter()-begin)*1000
    checkpoint=model.checkpoint();restored=PhraseStore.restore(json.loads(json.dumps(checkpoint)))
    if restored.snapshot()!=initial or restored.checkpoint()!=checkpoint:raise ValueError('phrase restart differs')
    first_per_thread={}
    for source in sources:first_per_thread.setdefault(source.thread,source)
    duplicate_ids=[]
    for source in first_per_thread.values():
        identity=source.id+'#phrase-duplicate-control'
        if identity in model.records:raise ValueError('duplicate fixture identity collision')
        model.upsert(PhraseSource(identity,source.thread,source.provider,source.text));duplicate_ids.append(identity)
    duplicate=model.snapshot()
    if duplicate_semantic(initial,set(initial['memberships']))!=duplicate_semantic(duplicate,set(initial['memberships'])):
        raise ValueError('duplicate lexical votes changed subjects')
    for identity in duplicate_ids:model.delete(identity)
    if semantic(model.snapshot())!=semantic(initial):raise ValueError('duplicate cleanup changed subjects')
    target=sources[0];model.delete(target.id);deleted=model.snapshot()
    if target.id in deleted['memberships'] or any(target.id in topic['sources'] for topic in deleted['topics']):raise ValueError('deleted evidence remains')
    if PhraseStore.restore(json.loads(json.dumps(model.checkpoint()))).snapshot()!=deleted:raise ValueError('deleted restart differs')
    model.upsert(target)
    if semantic(model.snapshot())!=semantic(initial):raise ValueError('reinsertion changed subjects')
    reverse_model=PhraseStore(model.namespace)
    for source in reversed(sources):reverse_model.upsert(source)
    reverse=reverse_model.snapshot()
    if semantic(reverse)!=semantic(initial):raise ValueError('arrival order changed final phrase subjects')
    if pins!={name:digest(Path(__file__).with_name(name)) for name in names} or digest(baseline)!=baseline_sha:raise ValueError('code/baseline drift')
    return {'schema_version':1,'scope':'phrase_subject_development_pilot_not_semantic_or_production_selection',
        'database_sha256':digest(database),'selection_sha256':digest(selection),'source_ledger_sha256':ledger,
        'baseline_sha256':baseline_sha,'code_pins':pins,'forward':initial,'reverse':reverse,
        'baseline_diagnostics':old['diagnostics'],'baseline_mean_thread_tags':sum(len(t['tags']) for t in old['threads'])/len(old['threads']),
        'checks':{'checkpoint_exact':True,'refreshed_restart_exact':True,'reverse_semantics_exact':True,
            'duplicate_controls':len(duplicate_ids),'duplicate_votes_invariant':True,'cleanup_exact':True,
            'real_source_deletion_absent':True,'deleted_restart_exact':True,'original_reinsertion_semantics_exact':True},
        'semantic_quality_verified':False,'production_selected':False,
        'runtime':{'initial_ingest_ms':ingestion,'initial_refresh_ms':refresh,'total_ms_after_load':(time.perf_counter()-start)*1000,
            'cpu_ms_after_load':(time.process_time()-cpu)*1000,'rusage_before_serialization_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['database','selection','baseline','output']:parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--compare',type=Path);args=parser.parse_args()
    result=experiment(args.database,args.selection,args.baseline)
    if args.compare and stable(json.loads(args.compare.read_text()))!=stable(result):raise ValueError('phrase repeat differs')
    with args.output.open('x') as file:json.dump(result,file,sort_keys=True,allow_nan=False)
    args.output.chmod(0o600)
    print(json.dumps({'complete':True,'diagnostics':result['forward']['diagnostics'],'checks':result['checks'],'runtime':result['runtime']}),flush=True)
