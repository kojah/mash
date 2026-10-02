"""Fixed lexical groups; corpus-IDF cTF labels versus balanced relevance."""
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
from relevance_labels import prepare,labels as relevance
from stable_frame_quality import label_cut
from stable_lexical_frame import State
from supported_labels import label,Profile


PINS=['qmsum_label_relevance.py','LABEL_RELEVANCE_PROTOCOL.md','relevance_labels.py','label_name_proxy.py','supported_labels.py',
    'label_noise.py','stable_frame_quality.py','stable_lexical_frame.py','calibration_metrics.py','discovery_inputs.py',
    'episode_blocks.py','qmsum.py','qmsum_hierarchy.py','captured_hierarchy.py','subject_labels.py','uv.lock']


def experiment(root):
    start=time.perf_counter();cpu=time.process_time();pins={n:digest(Path(__file__).with_name(n)) for n in PINS}
    def folder(n):return root/f'topic-qmsum-{n}-20261002'
    gp=folder('block-geometry-first')/'results.json';rp=root/'topic-qmsum-block-hierarchy-final-first-20261002.json'
    if digest(gp)!=GEOMETRY or digest(rp)!=REFERENCE:raise ValueError('immutable audited source/tree controls required')
    geometry=json.loads(gp.read_text());reference=json.loads(rp.read_text())
    data=read(SimpleNamespace(source=folder('author-training'),vectors=folder('semantic-vectors-first'),units=folder('semantic-units-final-first'),
        labels=folder('label-cleanup-first'),baseline=folder('lexical-context-final-first'),reference_manifest=root/'grinch-reference-20261001/manifest.json'))
    if any(x['source_sha256']!=data.source for x in [geometry,reference]):raise ValueError('source drift')
    banks=[];owners=[];states=[]
    for meeting,g in zip(data.meetings,geometry['records']):
        offset=g['source_offset'];size=len(meeting.turns);owner=np.full(size,-1,dtype=np.int64)
        for b in g['blocks']:owner[b['start']:b['end']+1]=b['ordinal']
        if not np.array_equal(owner>=0,data.ready[offset:offset+size]):raise ValueError('source support changed')
        bank=block_counts(data.counts[offset:offset+size],owner,len(g['blocks']));clean=bank.multiply([t not in EXCLUDED for t in data.terms]).tocsr();clean.eliminate_zeros()
        banks.append(bank);owners.append(owner);states.append([State.READY if clean[i].nnz else State.FALLBACK for i in range(bank.shape[0])])
    begin=time.perf_counter();corpus=prepare(sparse.vstack(banks,format='csr'),data.terms);background_ms=(time.perf_counter()-begin)*1000
    setup=(time.perf_counter()-start)*1000;records=[];timings=[];support=Counter()
    for meeting,g,bank,owner,state in zip(data.meetings,geometry['records'],banks,owners,states):
        old=next(r for r in reference['records'] if r['meeting']==meeting.ordinal and r['method']=='lexical_average');groups=label_cut(topology(old['tree']))
        begin=time.perf_counter();baseline=label(groups,bank,data.terms,state,corpus.background,Profile.DISTINCTIVE);baseline_ms=(time.perf_counter()-begin)*1000
        begin=time.perf_counter();candidate=relevance(groups,bank,data.terms,state,corpus);relevance_ms=(time.perf_counter()-begin)*1000
        chosen={'corpus_idf_ctfidf':[p.labels for p in baseline],'relevance_0_6':candidate}
        # Evaluation names enter only after all ranks are fixed.
        selected=np.flatnonzero(owner>=0).tolist();gold,_=components(meeting,selected);owned=owner[selected].tolist();metrics={}
        for name,words in chosen.items():metrics[name]=evaluate(groups,words,owned,gold,[t.name for t in meeting.topics])
        examples={}
        for name,words in chosen.items():
            examples[name]=[]
            for group,row in zip(groups,words):
                examples[name].append([{'term':term['term'],'blocks':[j for j in group if state[j]==State.READY and bank[j,corpus.background.positions[term['term']]]>0][:2]} for term in row])
        records.append({'meeting':meeting.ordinal,'groups':groups,'states':state,'labels':chosen,'name_proxy':metrics,'label_support_examples':examples})
        timings.append({'meeting':meeting.ordinal,'corpus_idf_ctfidf_ms':baseline_ms,'relevance_ms':relevance_ms})
        support.update(sources=len(owner),ready=len(selected),known=sum(t is not None for t in gold),blocks=len(g['blocks']),groups=len(groups))
        if meeting.ordinal%20==0:print(json.dumps({'processed_meetings':meeting.ordinal+1}),flush=True)
    if {k:support[k] for k in ['sources','ready','known','blocks']}!={'sources':88336,'ready':84434,'known':77892,'blocks':8196}:raise ValueError('full original support changed')
    summary={}
    for name in ['corpus_idf_ctfidf','relevance_0_6']:
        counts=Counter();groups=empty=weak=terms=0
        for r in records:
            counts.update({k:v for k,v in r['name_proxy'][name].items() if k!='rate'})
            for row in r['labels'][name]:groups+=1;empty+=not row;weak+=len(row)<2;terms+=len(row)
        summary[name]={'name_proxy':dict(counts)|{'rate':counts['hits']/counts['named_sources'] if counts['named_sources'] else None},'groups':groups,'empty_groups':empty,'weak_groups':weak,'label_terms':terms}
    if pins!={n:digest(Path(__file__).with_name(n)) for n in PINS}:raise ValueError('source changed during experiment')
    return {'schema_version':1,'scope':'fixed_group_label_relevance_not_semantic_quality_or_selection','source_sha256':data.source,'capture_sha256':data.capture,
        'geometry_sha256':GEOMETRY,'reference_sha256':REFERENCE,'support':dict(support),'summary':summary,'records':records,'code_pins':pins,
        'protocol':{'weight':.6,'new_inference':0,'llm_calls':0,'heldout_read':False,'production_selection':False,'full_ingestion_gate':False,'memberships_changed':False},
        'timings':{'setup_ms':setup,'background_ms':background_ms,'after_imports_ms':(time.perf_counter()-start)*1000,'cpu_ms':(time.process_time()-cpu)*1000,
            'rusage_peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,'stages':timings}}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--artifacts',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--compare',type=Path);a=p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    result=experiment(a.artifacts)
    if a.compare:
        old=json.loads(a.compare.read_text())
        if {k:v for k,v in old.items() if k!='timings'}!={k:v for k,v in result.items() if k!='timings'}:raise ValueError('complete non-timing repeat differs')
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps({'summary':result['summary'],'support':result['support']}),flush=True)


if __name__=='__main__':main()
