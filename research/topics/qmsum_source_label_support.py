"""Original-message versus physical-block support with fixed trees and scoring."""
import argparse
from collections import Counter
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
from label_name_proxy import evaluate
from label_noise import EXCLUDED
from qmsum_compressed_macro import REFERENCE
from qmsum_hierarchy import components
from qmsum_unique_blocks import GEOMETRY
from source_label_support import labels,Unit
from stable_frame_quality import label_cut
from stable_lexical_frame import State
from supported_labels import prepare_background


CONTROL='3145ec5f2bec206a7bf724a11dd15bf61509ca00524af252c0724eb4697a0697'
PINS=['qmsum_source_label_support.py','SOURCE_LABEL_SUPPORT_PROTOCOL.md','source_label_support.py','label_name_proxy.py',
    'supported_labels.py','label_noise.py','stable_frame_quality.py','stable_lexical_frame.py','calibration_metrics.py',
    'discovery_inputs.py','episode_blocks.py','qmsum.py','qmsum_hierarchy.py','captured_hierarchy.py','subject_labels.py','uv.lock']


def experiment(root):
    start=time.perf_counter();cpu=time.process_time();pins={n:digest(Path(__file__).with_name(n)) for n in PINS}
    def folder(n):return root/f'topic-qmsum-{n}-20261002'
    gp=folder('block-geometry-first')/'results.json';rp=root/'topic-qmsum-block-hierarchy-final-first-20261002.json'
    cp=root/'topic-qmsum-label-relevance-first-20261002.json'
    if digest(gp)!=GEOMETRY or digest(rp)!=REFERENCE or digest(cp)!=CONTROL:raise ValueError('immutable source/tree/label controls required')
    geometry=json.loads(gp.read_text());reference=json.loads(rp.read_text());control=json.loads(cp.read_text())
    data=read(SimpleNamespace(source=folder('author-training'),vectors=folder('semantic-vectors-first'),units=folder('semantic-units-final-first'),
        labels=folder('label-cleanup-first'),baseline=folder('lexical-context-final-first'),reference_manifest=root/'grinch-reference-20261001/manifest.json'))
    if any(x['source_sha256']!=data.source for x in [geometry,reference,control]):raise ValueError('source drift')
    banks=[];owners=[];states=[]
    for meeting,g in zip(data.meetings,geometry['records']):
        offset=g['source_offset'];size=len(meeting.turns);owner=np.full(size,-1,dtype=np.int64)
        for b in g['blocks']:owner[b['start']:b['end']+1]=b['ordinal']
        if not np.array_equal(owner>=0,data.ready[offset:offset+size]):raise ValueError('original support changed')
        bank=block_counts(data.counts[offset:offset+size],owner,len(g['blocks']));clean=bank.multiply([t not in EXCLUDED for t in data.terms]).tocsr();clean.eliminate_zeros()
        banks.append(bank);owners.append(owner);states.append([State.READY if clean[i].nnz else State.FALLBACK for i in range(bank.shape[0])])
    begin=time.perf_counter();background=prepare_background(sparse.vstack(banks,format='csr'),data.terms);background_ms=(time.perf_counter()-begin)*1000
    setup=(time.perf_counter()-start)*1000;records=[];timings=[];support=Counter()
    for meeting,g,bank,owner,state,previous in zip(data.meetings,geometry['records'],banks,owners,states,control['records']):
        tree=next(r for r in reference['records'] if r['meeting']==meeting.ordinal and r['method']=='lexical_average')['tree']
        groups=label_cut(topology(tree));offset=g['source_offset'];counts=data.counts[offset:offset+len(owner)]
        ids=list(range(offset,offset+len(owner)));chosen={};stages={}
        for unit in Unit:
            begin=time.perf_counter();chosen[unit]=labels(groups,counts,owner,ids,data.terms,state,background,unit);stages[unit]=(time.perf_counter()-begin)*1000
        projection=[[{k:v for k,v in t.items() if k!='member_sources'} for t in row] for row in chosen[Unit.BLOCK]]
        if groups!=previous['groups'] or state!=previous['states'] or projection!=previous['labels']['corpus_idf_ctfidf']:raise ValueError('baseline groups/ranks/scores/support drift')
        selected=np.flatnonzero(owner>=0).tolist();gold,_=components(meeting,selected);owned=owner[selected].tolist()
        metrics={unit:evaluate(groups,rows,owned,gold,[t.name for t in meeting.topics]) for unit,rows in chosen.items()}
        provenance={}
        for unit,rows in chosen.items():
            provenance[unit]=[]
            for group,row in zip(groups,rows):
                eligible=np.flatnonzero(np.isin(owner,[j for j in group if state[j]==State.READY]))
                provenance[unit].append([{'term':t['term'],'original_sources':[ids[j] for j in eligible if counts[j,background.positions[t['term']]]>0][:2]} for t in row])
        records.append({'meeting':meeting.ordinal,'source_offset':offset,'groups':groups,'states':state,'labels':chosen,'name_proxy':metrics,
            'support_examples':provenance,'group_ready_sources':[int(np.isin(owner,group).sum()) for group in groups]})
        timings.append({'meeting':meeting.ordinal,'label_ms':stages});support.update(sources=len(owner),ready=len(selected),known=sum(t is not None for t in gold),blocks=len(g['blocks']),groups=len(groups))
        if meeting.ordinal%20==0:print(json.dumps({'processed_meetings':meeting.ordinal+1}),flush=True)
    if dict(support)!={'sources':88336,'ready':84434,'known':77892,'blocks':8196,'groups':1296}:raise ValueError('full original support changed')
    summary={}
    for unit in Unit:
        metrics=Counter();diagnostics=Counter()
        for r in records:
            metrics.update({k:v for k,v in r['name_proxy'][unit].items() if k!='rate'})
            for group,row,old,n in zip(r['groups'],r['labels'][unit],r['labels'][Unit.BLOCK],r['group_ready_sources']):
                diagnostics.update(groups=1,empty_groups=int(not row),weak_groups=int(len(row)<2),label_terms=len(row),
                    recovered_empty_groups=int(not old and bool(row)),recovered_two_term_groups=int(len(old)<2<=len(row)),
                    single_block_groups=int(len(group)==1),single_original_source_groups=int(n==1))
        summary[unit]={'name_proxy':dict(metrics)|{'rate':metrics['hits']/metrics['named_sources']},'diagnostics':dict(diagnostics)}
    if pins!={n:digest(Path(__file__).with_name(n)) for n in PINS}:raise ValueError('source changed during experiment')
    return {'schema_version':1,'scope':'original_source_label_support_not_semantic_quality_or_selection','source_sha256':data.source,'capture_sha256':data.capture,
        'geometry_sha256':GEOMETRY,'reference_sha256':REFERENCE,'control_sha256':CONTROL,'support':dict(support),'summary':summary,'records':records,'code_pins':pins,
        'protocol':{'new_inference':0,'llm_calls':0,'heldout_read':False,'production_selection':False,'full_ingestion_gate':False,'memberships_changed':False,'scoring_changed':False},
        'timings':{'setup_ms':setup,'background_ms':background_ms,'after_imports_ms':(time.perf_counter()-start)*1000,'cpu_ms':(time.process_time()-cpu)*1000,
            'rusage_peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,'stages':timings}}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--artifacts',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--compare',type=Path);a=p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    result=experiment(a.artifacts)
    if a.compare and {k:v for k,v in json.loads(a.compare.read_text()).items() if k!='timings'}!={k:v for k,v in result.items() if k!='timings'}:raise ValueError('complete non-timing repeat differs')
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps({'summary':result['summary'],'support':result['support']}),flush=True)


if __name__=='__main__':main()
