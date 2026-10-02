"""Matched frozen phrase allocation control on reviewed and wider sources."""
import argparse
import json
from pathlib import Path
import resource
import time
from phrase_review_evaluation import load,evaluate,digest
from topic_cohort_demo import load as cohort,PINS
from topic_phrases import PhraseSource
from topic_phrase_store import PhraseStore
from phrase_selection import Selection


def stable(result):
    result=dict(result);result.pop('runtime');return json.dumps(result,sort_keys=True,allow_nan=False,separators=(',',':'))


def semantic(snapshot):
    result=dict(snapshot);result.pop('revision');result.pop('versions');return result


def evidence_projection(snapshot,original):
    return {'topics':[{key:value for key,value in topic.items() if key!='sources'} for topic in snapshot['topics']],
        'threads':[{key:value for key,value in thread.items() if key!='sources'} for thread in snapshot['threads']],
        'memberships':{sid:tags for sid,tags in snapshot['memberships'].items() if sid in original}}


def model_for(sources,namespace):
    model=PhraseStore(namespace)
    for source in sources:model.upsert(source)
    return model


def lifecycle(model,sources,initial,selection=Selection.SOURCE_COVERAGE):
    checkpoint=json.loads(json.dumps(model.checkpoint()))
    if PhraseStore.restore(checkpoint).snapshot(selection)!=initial:raise ValueError('selected-policy restart differs')
    first={}
    for source in sources:first.setdefault(source.thread,source)
    duplicates=[]
    for source in first.values():
        identity=source.id+'#unit-duplicate'
        if identity in model.records:raise ValueError('duplicate fixture collision')
        model.upsert(PhraseSource(identity,source.thread,source.provider,source.text));duplicates.append(identity)
    if evidence_projection(model.snapshot(selection),set(initial['memberships']))!=evidence_projection(initial,set(initial['memberships'])):
        raise ValueError('duplicate vote changed source allocation')
    for identity in duplicates:model.delete(identity)
    if semantic(model.snapshot(selection))!=semantic(initial):raise ValueError('duplicate cleanup differs')
    target=sources[0];model.delete(target.id);deleted=model.snapshot(selection)
    if target.id in deleted['memberships'] or any(target.id in topic['sources'] for topic in deleted['topics']):raise ValueError('deleted source survives')
    if PhraseStore.restore(json.loads(json.dumps(model.checkpoint()))).snapshot(selection)!=deleted:raise ValueError('deleted restart differs')
    model.upsert(target)
    if semantic(model.snapshot(selection))!=semantic(initial):raise ValueError('reinsertion changes final semantics')
    reverse=model_for(list(reversed(sources)),model.namespace).snapshot(selection)
    if semantic(reverse)!=semantic(initial):raise ValueError('reverse source allocation differs')
    return {'refreshed_restart_exact':True,'duplicate_controls':len(duplicates),'duplicate_vote_invariant':True,
            'duplicate_cleanup_exact':True,'real_delete_absent':True,'deleted_restart_exact':True,'reinsertion_exact':True,'reverse_semantics_exact':True}


def referential_only(snapshot,packet,review):
    cases={case['thread']:case for case in review['cases']};kinds={}
    for case in packet['cases']:
        for turn,judgment in zip(case['turns'],cases[case['thread']]['admission_judgments']):
            sid=json.dumps([case['thread'],turn['id']],ensure_ascii=False,separators=(',',':'));kinds[sid]=judgment['kind']
    return sum(all(kinds[sid]=='referential' for sid in topic['sources']) for topic in snapshot['topics'])


def experiment(args):
    names=PINS+['phrase_review_evaluation.py','topic_phrases.py','topic_phrase_store.py','phrase_selection.py','phrase_selection_control.py']
    pins={name:digest(Path(__file__).with_name(name)) for name in names}
    hashes={name:digest(getattr(args,name)) for name in ['packet','review','database','selection','review_baseline','cohort_baseline']}
    if args.allocation_baseline:hashes['allocation_baseline']=digest(args.allocation_baseline)
    if args.phatic_filter and not args.allocation_baseline:raise ValueError('frozen allocation control required for phatic arm')
    modes=[Selection.WHOLE_THREAD,Selection.SOURCE_COVERAGE]
    if args.phatic_filter:modes.append(Selection.PHATIC_COVERAGE)
    sources,subjects,policy=load(args.packet,args.review)
    old_review=json.loads(args.review_baseline.read_text());old_cohort=json.loads(args.cohort_baseline.read_text())
    start=time.perf_counter();cpu=time.process_time();outputs={};timings={}
    for scope,points,prior in [('review',sources,old_review['snapshot'])]:
        model=model_for(points,prior['namespace']);arms={}
        for mode in modes:
            begin=time.perf_counter();snapshot=model.snapshot(mode);rows,summary=evaluate(snapshot,subjects)
            if mode==Selection.WHOLE_THREAD and (snapshot!=prior or summary!=old_review['summary']):raise ValueError('whole-thread review control drift')
            arms[mode]={'snapshot':snapshot,'subjects':rows,'summary':summary,
                'topics_with_only_reviewed_referential_evidence':referential_only(snapshot,json.loads(args.packet.read_text()),json.loads(args.review.read_text()))}
            timings[scope+'_'+mode]=(time.perf_counter()-begin)*1000
        outputs[scope]=arms
        outputs['review_lifecycle']=lifecycle(model,points,arms[Selection.SOURCE_COVERAGE]['snapshot'])
        if args.phatic_filter:outputs['phatic_review_lifecycle']=lifecycle(model_for(points,prior['namespace']),points,arms[Selection.PHATIC_COVERAGE]['snapshot'],Selection.PHATIC_COVERAGE)
    broad,_,_,_,ledger=cohort(args.database,args.selection)
    broad=[PhraseSource(s.id,s.thread,s.provider,s.text) for s in broad];model=model_for(broad,old_cohort['forward']['namespace']);arms={}
    for mode in modes:
        begin=time.perf_counter();snapshot=model.snapshot(mode)
        if mode==Selection.WHOLE_THREAD and snapshot!=old_cohort['forward']:raise ValueError('whole-thread broad control drift')
        arms[mode]=snapshot;timings['cohort_'+mode]=(time.perf_counter()-begin)*1000
    outputs['cohort']=arms
    outputs['cohort_lifecycle']=lifecycle(model,broad,arms[Selection.SOURCE_COVERAGE])
    if args.phatic_filter:outputs['phatic_cohort_lifecycle']=lifecycle(model_for(broad,old_cohort['forward']['namespace']),broad,arms[Selection.PHATIC_COVERAGE],Selection.PHATIC_COVERAGE)
    if args.allocation_baseline:
        previous=json.loads(args.allocation_baseline.read_text())
        for scope in ['review','cohort']:
            for mode in [Selection.WHOLE_THREAD,Selection.SOURCE_COVERAGE]:
                if outputs[scope][mode]!=previous['outputs'][scope][mode]:raise ValueError('frozen allocation arm differs')
        for key in ['review_lifecycle','cohort_lifecycle']:
            if outputs[key]!=previous['outputs'][key]:raise ValueError('frozen allocation lifecycle differs')
    if pins!={name:digest(Path(__file__).with_name(name)) for name in names} or hashes!={name:digest(getattr(args,name)) for name in hashes}:
        raise ValueError('input/code drift')
    return {'schema_version':1,'scope':'fixed_phatic_candidate_control_not_semantic_selection' if args.phatic_filter else 'fixed_source_unit_allocation_development_control_not_semantic_selection',
        'code_pins':pins,'input_hashes':hashes,'review_policy':policy,'source_ledger_sha256':ledger,'outputs':outputs,
        'whole_thread_controls_exact':True,'semantic_quality_verified':False,'production_selected':False,
        'runtime':{'refresh_and_review_ms':timings,'total_ms':(time.perf_counter()-start)*1000,
            'cpu_ms':(time.process_time()-cpu)*1000,'rusage_before_serialization_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['packet','review','database','selection','review_baseline','cohort_baseline','output']:
        parser.add_argument('--'+name.replace('_','-'),type=Path,required=True)
    parser.add_argument('--compare',type=Path);parser.add_argument('--allocation-baseline',type=Path)
    parser.add_argument('--phatic-filter',action='store_true');args=parser.parse_args();result=experiment(args)
    if args.compare and stable(json.loads(args.compare.read_text()))!=stable(result):raise ValueError('source allocation repeat differs')
    with args.output.open('x') as file:json.dump(result,file,sort_keys=True,allow_nan=False)
    args.output.chmod(0o600)
    print(json.dumps({'complete':True,'review':{mode:arm['summary'] for mode,arm in result['outputs']['review'].items()},
        'cohort':{mode:snapshot['diagnostics'] for mode,snapshot in result['outputs']['cohort'].items()},'runtime':result['runtime']}),flush=True)
