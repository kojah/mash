"""Frozen cut/label factorial on existing trees; no clustering fit or inference."""
import argparse
from collections import Counter
from dataclasses import asdict
import json
from pathlib import Path
import resource
import time
from types import SimpleNamespace
import numpy as np
from scipy import sparse
from calibration_metrics import topology
from discovery_inputs import read,digest
from episode_blocks import block_counts
from qmsum_calibration import PINNAMES as CALIBRATION_PINS,TF_REFERENCE
from qmsum_hierarchy import components
from qmsum_unique_blocks import GEOMETRY
from semantic_cut import select
from stable_frame_quality import label_cut
from supported_labels import label,prepare_background,Profile,Support
from topic_pair_metrics import evaluate


CALIBRATION='d99fe9802bb958211a6055c1222b26546da9d2914db2cb0a0f0c843385c64305'
PINNAMES=sorted(set(CALIBRATION_PINS+['qmsum_semantic_cut.py','SEMANTIC_CUT_PROTOCOL.md',
    'semantic_cut.py','supported_labels.py','topic_pair_metrics.py']))


def summarize(rows):
    out={}
    for scope in ['all','far','crossspan']:
        totals={k:sum(r[scope][k] for r in rows) for k in ['gold_pairs','predicted_pairs','true_positive_pairs']}
        g,p,t=(totals[k] for k in ['gold_pairs','predicted_pairs','true_positive_pairs'])
        out[scope]={**totals,'precision':t/p if p else None,'recall':t/g if g else None,'f1':2*t/(p+g) if p+g else None}
    coverage=Counter()
    for r in rows:coverage.update({k:v for k,v in r['coverage'].items() if k!='group_sizes'})
    out['coverage']=dict(coverage)
    out['group_sizes']=sorted(size for r in rows for size in r['coverage']['group_sizes'])
    return out


def experiment(root):
    start=time.perf_counter();cpu=time.process_time();pins={n:digest(Path(__file__).with_name(n)) for n in PINNAMES}
    def folder(n):return root/f'topic-qmsum-{n}-20261002'
    gp=folder('block-geometry-first')/'results.json'
    paths={'fixed_tf':root/'topic-qmsum-stable-frame-quality-first-20261002.json',
        'calibrated_32':folder('calibration-final-first')/'results.json'}
    expected={'fixed_tf':TF_REFERENCE,'calibrated_32':CALIBRATION}
    if digest(gp)!=GEOMETRY or any(digest(p)!=expected[k] for k,p in paths.items()):raise ValueError('immutable audited input required')
    geometry=json.loads(gp.read_text());controls={k:json.loads(p.read_text()) for k,p in paths.items()}
    for n,pin in geometry['code_pins'].items():
        if digest(Path(__file__).with_name(n))!=pin:raise ValueError('frozen source geometry drift')
    vector_path=gp.parent/'block-directions.npy';vector_pin=geometry['files'][vector_path.name]['sha256']
    if digest(vector_path)!=vector_pin:raise ValueError('semantic vector drift')
    vectors=np.load(vector_path,mmap_mode='r',allow_pickle=False)
    args=SimpleNamespace(source=folder('author-training'),vectors=folder('semantic-vectors-first'),units=folder('semantic-units-final-first'),
        labels=folder('label-cleanup-first'),baseline=folder('lexical-context-final-first'),reference_manifest=root/'grinch-reference-20261001/manifest.json')
    data=read(args)
    if any(x['source_sha256']!=data.source for x in [geometry,*controls.values()]):raise ValueError('source drift')
    banks=[];owners=[]
    for meeting,row in zip(data.meetings,geometry['records']):
        owner=np.full(len(meeting.turns),-1,dtype=np.int64)
        for b in row['blocks']:owner[b['start']:b['end']+1]=b['ordinal']
        if not np.array_equal(owner>=0,data.ready[row['source_offset']:row['source_offset']+len(owner)]):raise ValueError('ready source drift')
        banks.append(block_counts(data.counts[row['source_offset']:row['source_offset']+len(owner)],owner,len(row['blocks'])));owners.append(owner)
    background=prepare_background(sparse.vstack(banks,format='csr'),data.terms)
    setup_ms=(time.perf_counter()-start)*1000;records=[];timings=[];support=Counter()
    indexed={k:{(r['meeting'],r['order']):r for r in c['records'] if r['policy']=='static_noise_filtered_tf' and r['kind']=='hashed'} for k,c in controls.items()}
    for meeting,row,bank,owner in zip(data.meetings,geometry['records'],banks,owners):
        selected=np.flatnonzero(owner>=0).tolist();owned=owner[selected].tolist();gold,parts=components(meeting,selected)
        lo=row['block_directions_offset'];v=vectors[lo:lo+len(row['blocks'])]
        for frame in controls:
            for order in ['forward','reverse']:
                old=indexed[frame][meeting.ordinal,order];tree=topology(old['tree']);begin=time.perf_counter();cut=select(tree,v)
                cut_ms=(time.perf_counter()-begin)*1000
                for name,groups in [('fixed_eight',label_cut(tree)),('semantic',cut.groups)]:
                    proposed=evaluate(groups,owned,gold,parts,selected)
                    for profile in Profile:
                        begin=time.perf_counter();proposals=label(groups,bank,data.terms,old['states'],background,profile)
                        label_ms=(time.perf_counter()-begin)*1000
                        usable=[p.blocks for p in proposals if p.support==Support.SUPPORTED]
                        metrics=evaluate(usable,owned,gold,parts,selected);examples=[]
                        for p in proposals:
                            # Freeze arithmetic and tie rule; do not silently switch to pairwise reductions.
                            mean=v[p.blocks].mean(axis=0);scores=v[p.blocks]@mean
                            representative=p.blocks[int(np.argmax(scores))];b=row['blocks'][representative]
                            examples.append({**asdict(p),'representative_block':representative,'representative_score':float(scores.max()),
                                'representative_source':{'start':b['start'],'end':b['end'],'text':'\n'.join(t.text for t in meeting.turns[b['start']:b['end']+1])}})
                        records.append({'meeting':meeting.ordinal,'frame':frame,'order':order,'cut':name,'profile':profile,
                            'semantic_cut':asdict(cut) if name=='semantic' and profile==Profile.STATIC else None,
                            'proposals':examples,'proposed_metrics':proposed,'usable_metrics':metrics,'unchanged_uncut_metrics':old['metrics']})
                        timings.append({'meeting':meeting.ordinal,'frame':frame,'order':order,'cut':name,'profile':profile,'cut_ms':cut_ms,'label_ms':label_ms})
        support.update(sources=len(owner),ready=len(selected),known=sum(g is not None for g in gold),blocks=len(row['blocks']))
        if meeting.ordinal%20==0:print(json.dumps({'processed_meetings':meeting.ordinal+1}),flush=True)
    if support!={'sources':88336,'ready':84434,'known':77892,'blocks':8196}:raise ValueError('full original source support required')
    summaries=[]
    for frame in controls:
        for order in ['forward','reverse']:
            for name in ['fixed_eight','semantic']:
                for profile in Profile:
                    rows=[r for r in records if (r['frame'],r['order'],r['cut'],r['profile'])==(frame,order,name,profile)]
                    entry={'frame':frame,'order':order,'cut':name,'profile':profile}
                    for phase in ['proposed','usable']:
                        s=summarize([r[f'{phase}_metrics'] for r in rows])
                        if [s[k]['gold_pairs'] for k in ['all','far','crossspan']]!=[7978214,7923483,84323]:raise ValueError('original positive pair denominator changed')
                        entry[phase]=s
                    summaries.append(entry)
    if pins!={n:digest(Path(__file__).with_name(n)) for n in PINNAMES}:raise ValueError('source changed during experiment')
    return {'schema_version':1,'scope':'frozen_semantic_cut_factorial_not_production_selection','source_sha256':data.source,'capture_sha256':data.capture,
        'control_sha256':expected,'geometry_sha256':GEOMETRY,'vectors_sha256':vector_pin,'support':dict(support),'code_pins':pins,'summary':summaries,'records':records,
        'protocol':{'new_inference':0,'llm_calls':0,'heldout_read':False,'production_selection':False,'global_online_labels':False,'full_ingestion_gate':False},
        'timings':{'setup_ms':setup_ms,'after_imports_ms':(time.perf_counter()-start)*1000,'cpu_ms':(time.process_time()-cpu)*1000,
            'rusage_peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,'stages':timings}}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--artifacts',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--compare',type=Path);a=p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    result=experiment(a.artifacts)
    if a.compare:
        old=json.loads(a.compare.read_text())
        if {k:v for k,v in old.items() if k!='timings'}!={k:v for k,v in result.items() if k!='timings'}:raise ValueError('complete non-timing repeat differs')
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps({'support':result['support'],'comparisons':len(result['summary'])}),flush=True)


if __name__=='__main__':main()
