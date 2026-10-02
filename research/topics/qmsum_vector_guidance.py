"""Matched scalar/vector guidance on identical frozen query score arrays."""
import argparse
import json
from pathlib import Path
import resource
import time
from types import SimpleNamespace
import numpy as np
from discovery_inputs import read,digest
from navigation_features import build
from query_navigation import query,rank
from soft_topic_guidance import retrieve as scalar
from vector_topic_guidance import retrieve as vectorized


REFERENCE='1a4c6c992a2bd18213ae5a53bf20235d934db2e16eea4de7c167359f7b1ea0dc'
PINS=['qmsum_vector_guidance.py','VECTOR_GUIDANCE_PROTOCOL.md','vector_topic_guidance.py','soft_topic_guidance.py',
    'navigation_features.py','query_navigation.py','discovery_inputs.py','episode_blocks.py','label_noise.py','stable_lexical_frame.py','qmsum.py','uv.lock']


def experiment(root):
    start=time.perf_counter();cpu=time.process_time();pins={n:digest(Path(__file__).with_name(n)) for n in PINS}
    rp=root/'topic-qmsum-soft-guidance-first-20261002.json'
    if digest(rp)!=REFERENCE:raise ValueError('audited guidance reference required')
    reference=json.loads(rp.read_text())
    for name,pin in reference['code_pins'].items():
        if digest(Path(__file__).with_name(name))!=pin:raise ValueError('frozen reference source drift')
    cp=root/'topic-qmsum-query-navigation-first-20261002.json'
    if digest(cp)!=reference['control_sha256']:raise ValueError('query control drift')
    control=json.loads(cp.read_text())
    def folder(n):return root/f'topic-qmsum-{n}-20261002'
    data=read(SimpleNamespace(source=folder('author-training'),vectors=folder('semantic-vectors-first'),units=folder('semantic-units-final-first'),
        labels=folder('label-cleanup-first'),baseline=folder('lexical-context-final-first'),reference_manifest=root/'grinch-reference-20261001/manifest.json'))
    if data.source!=reference['source_sha256']:raise ValueError('original source drift')
    sp=root/'topic-qmsum-source-support-first-20261002.json'
    if digest(sp)!=control['control_sha256']:raise ValueError('source ownership control drift')
    support=json.loads(sp.read_text());gp=folder('block-geometry-first')/'results.json'
    if digest(gp)!=support['geometry_sha256']:raise ValueError('source geometry drift')
    geometry=json.loads(gp.read_text());groups=control['groups'];owners=np.full(len(data.ready),-1,dtype=np.int64);source_meeting=np.full(len(owners),-1,dtype=np.int64)
    for meeting,g in zip(data.meetings,geometry['records']):
        offset=g['source_offset'];source_meeting[offset:offset+len(meeting.turns)]=meeting.ordinal
        assignment={b:v['id'] for v in groups if v['meeting']==meeting.ordinal for b in v['blocks']}
        for b in g['blocks']:owners[offset+b['start']:offset+b['end']+1]=assignment[b['ordinal']]
    if not np.array_equal(owners>=0,data.ready) or (source_meeting<0).any():raise ValueError('frozen full source ownership required')
    begin=time.perf_counter();features=build(data.counts,data.idf,data.terms,owners,len(groups));features_ms=(time.perf_counter()-begin)*1000
    setup_ms=(time.perf_counter()-start)*1000;positions={t:i for i,t in enumerate(data.terms)};ready_ids=np.flatnonzero(data.ready);gids=np.arange(len(groups),dtype=np.int64)
    group_meeting=np.array([g['meeting'] for g in groups]);timings=[];cases_checked=0
    for r in reference['records']:
        meeting=data.meetings[r['meeting']];q=meeting.specific_queries[r['query']]
        begin=time.perf_counter();direction,metadata=query(q.query,positions,data.idf);direction.sort_indices();sources,topics=features.score(direction);projection_ms=(time.perf_counter()-begin)*1000
        if metadata!=r['query_terms']:raise ValueError('query projection drift')
        scopes={'meeting':(ready_ids[source_meeting[ready_ids]==meeting.ordinal],gids[group_meeting==meeting.ordinal]),'global':(ready_ids,gids)};stage={}
        for scope,(ids,group_ids) in scopes.items():
            if rank(sources[ids],ids,20)!=r['cases'][scope]['direct_sources']['returned_sources']:raise ValueError('direct scores/identities drift')
            args=(sources[ids],ids,owners[ids],topics[group_ids],group_ids)
            begin=time.perf_counter();old=scalar(*args);scalar_ms=(time.perf_counter()-begin)*1000
            begin=time.perf_counter();new=vectorized(*args);vector_ms=(time.perf_counter()-begin)*1000
            expected={k:v for k,v in r['cases'][scope]['parent_rrf'].items() if k!='metrics'}
            if old!=expected or new!=old:raise ValueError('complete source/rank/score/candidate record differs')
            stage[scope]={'scalar_ms':scalar_ms,'vector_ms':vector_ms};cases_checked+=1
        timings.append({'meeting':meeting.ordinal,'query':q.ordinal,'projection_ms':projection_ms,'scopes':stage})
        if len(timings)%200==0:print(json.dumps({'matched_queries':len(timings)}),flush=True)
    if len(timings)!=1095 or cases_checked!=2190 or digest(rp)!=REFERENCE or pins!={n:digest(Path(__file__).with_name(n)) for n in PINS}:raise ValueError('full equivalence/code controls changed')
    # Every returned identity/rank/score equals the audited reference, so its
    # original-source metric ledger is preserved, then independently re-audited.
    result={k:v for k,v in reference.items() if k not in ['timings','code_pins','scope']}
    result.update(scope='exact_vector_guidance_not_new_quality_or_production_selection',reference_capture_sha256=REFERENCE,
        equivalence={'queries':len(timings),'scoped_pairs':cases_checked,'all_record_fields_exact':True,'candidate_pruning':False},code_pins=pins,
        timings={'setup_ms':setup_ms,'features_ms':features_ms,'after_imports_ms':(time.perf_counter()-start)*1000,'cpu_ms':(time.process_time()-cpu)*1000,
            'rusage_peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,'queries':timings})
    return result


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--artifacts',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--compare',type=Path);a=p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    result=experiment(a.artifacts)
    if a.compare and {k:v for k,v in json.loads(a.compare.read_text()).items() if k!='timings'}!={k:v for k,v in result.items() if k!='timings'}:raise ValueError('complete non-timing repeat differs')
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps(result['equivalence']),flush=True)


if __name__=='__main__':main()
