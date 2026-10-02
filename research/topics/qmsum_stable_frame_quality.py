"""Future-free lexical topic geometry on unchanged original source blocks."""
import argparse
from collections import Counter
import gc
import json
import math
from pathlib import Path
import resource
import time
import numpy as np
from scipy import sparse
from threadpoolctl import threadpool_limits
from block_hierarchy import evaluate
from discovery_inputs import digest
from episode_blocks import block_counts
from online_tree import Mode
from qmsum import load
from qmsum_hierarchy import components
from qmsum_sparse_average import fit
from qmsum_compressed_macro import REFERENCE
from qmsum_unique_blocks import GEOMETRY
from qmsum_block_hierarchy import stored_tree
from stable_lexical_frame import Policy,fingerprint
from stable_frame_quality import identity_frame,cosine_errors,label_examples


PREFLIGHT='1c989b0d905270a3b667e6a62c4f12e73ef8cce50e4a075a47a55675731ead86'
LABELS='064ff3731955b515a07e12e3897102e2cd9d137ac91c43ec222e393a2e492d08'


def experiment(root):
    start=time.perf_counter();cpu=time.process_time()
    def folder(name):return root/('topic-qmsum-'+name+'-20261002')
    pre=folder('stable-frame-preflight-first');gpath=folder('block-geometry-first')/'results.json'
    reference_path=root/'topic-qmsum-block-hierarchy-final-first-20261002.json'
    labels_path=folder('label-cleanup-first');baseline=folder('lexical-context-final-first')
    if digest(pre/'results.json')!=PREFLIGHT or digest(gpath)!=GEOMETRY or digest(reference_path)!=REFERENCE or digest(labels_path/'results.json')!=LABELS:
        raise ValueError('immutable full-source captures required')
    preflight=json.loads((pre/'results.json').read_text());geometry=json.loads(gpath.read_text());reference=json.loads(reference_path.read_text())
    for name,pin in preflight['code_pins'].items():
        if digest(Path(__file__).with_name(name))!=pin:raise ValueError('frozen frame drift')
    for name,pin in geometry['code_pins'].items():
        if digest(Path(__file__).with_name(name))!=pin:raise ValueError('frozen geometry drift')
    for name,pin in preflight['files'].items():
        if digest(pre/name)!=pin:raise ValueError('encoded bank drift')
    banks={p:sparse.csr_matrix(tuple(np.load(pre/f'{p}-{k}.npy',allow_pickle=False) for k in ['data','indices','indptr']),shape=(8196,16384)) for p in Policy}
    source_capture,source,meetings=load(folder('author-training'))
    if any(c['source_sha256']!=source for c in [preflight,geometry,reference]):raise ValueError('source drift')
    ledger=json.loads((labels_path/'results.json').read_text());schema=ledger['matrices']['exact_markers']
    for name,pin in schema['files'].items():
        if digest(labels_path/name)!=pin:raise ValueError('source-count drift')
    source_counts=sparse.csr_matrix(tuple(np.load(labels_path/f'exact_markers-{k}.npy',allow_pickle=False) for k in ['data','indices','indptr']),shape=tuple(schema['shape']))
    old=json.loads((baseline/'results.json').read_text())
    if digest(baseline/'results.json')!=ledger['baseline_sha256'] or digest(baseline/'vocabulary.json')!=old['vocabulary_sha256']:raise ValueError('term identity oracle drift')
    terms=json.loads((baseline/'vocabulary.json').read_text());del ledger,old;gc.collect()
    setup_ms=(time.perf_counter()-start)*1000;records=[];timings=[];distortions=[];support=Counter()
    with threadpool_limits(limits=1):
        for meeting,row in zip(meetings,geometry['records']):
            offset=row['source_offset'];blocks=row['blocks'];size=len(meeting.turns);owner=np.full(size,-1,dtype=np.int64)
            for b in blocks:owner[b['start']:b['end']+1]=b['ordinal']
            selected=np.flatnonzero(owner>=0).tolist();gold,parts=components(meeting,selected);owned=owner[selected].tolist()
            counts=block_counts(source_counts[offset:offset+size],owner,len(blocks));lo=row['block_directions_offset']
            expected=[{'meeting':meeting.ordinal,'block':b['ordinal'],'start':b['start'],'end':b['end'],'source_offset':offset} for b in blocks]
            if expected!=preflight['records'][lo:lo+len(blocks)]:raise ValueError('frame source ownership drift')
            for policy in Policy:
                identity,states=identity_frame(counts,terms,policy);hashed=banks[policy][lo:lo+len(blocks)]
                if any(s=='empty' for s in states) or fingerprint(policy)!=preflight['summary'][policy]['frame_sha256']:raise ValueError('complete original frame support required')
                expected_fallback=[lo+i for i,s in enumerate(states) if s=='filtered_empty_fallback']
                if expected_fallback!=[i for i in preflight['summary'][policy]['fallback_block_ids'] if lo<=i<lo+len(blocks)]:raise ValueError('fallback drift')
                distortions.append({'meeting':meeting.ordinal,'policy':policy,**cosine_errors(hashed,identity)})
                for kind,vectors in [('hashed',hashed),('identity_oracle',identity)]:
                    for order,reverse in [('forward',False),('reverse',True)]:
                        tree,state,t=fit(vectors,'lexical',Mode.ROTATE,reverse)
                        begin=time.perf_counter();metrics=evaluate(tree,owned,gold,parts,selected,len(meeting.topics));t['evaluation_ms']=(time.perf_counter()-begin)*1000
                        begin=time.perf_counter();labels=label_examples(tree,vectors,counts,terms,states);t['labels_ms']=(time.perf_counter()-begin)*1000
                        for label in labels:
                            b=blocks[label['representative_block']];text='\n'.join(turn.text for turn in meeting.turns[b['start']:b['end']+1])
                            label['representative_source']={'start':b['start'],'end':b['end'],'text':text}
                        records.append({'meeting':meeting.ordinal,'split':row['split'],'policy':policy,'kind':kind,'order':order,
                            'blocks':len(blocks),'states':states,'state':state,'tree':stored_tree(tree),'metrics':metrics,'display_labels':labels})
                        timings.append({'meeting':meeting.ordinal,'policy':policy,'kind':kind,'order':order,**t})
            support.update(sources=size,ready=len(selected),known=sum(g is not None for g in gold),blocks=len(blocks))
            if meeting.ordinal%10==0:print(json.dumps({'processed_meetings':meeting.ordinal+1}),flush=True)
    if support!={'sources':88336,'ready':84434,'known':77892,'blocks':8196}:raise ValueError('complete original support required')
    summary={}
    for policy in Policy:
        summary[policy]={}
        for kind in ['hashed','identity_oracle']:
            summary[policy][kind]={}
            for order in ['forward','reverse']:
                rows=[r for r in records if (r['policy'],r['kind'],r['order'])==(policy,kind,order)];entry={}
                for scope in ['all','far','crossspan']:
                    values=[r['metrics'][scope] for r in rows];pairs=sum(v['pairs'] for v in values);weight=math.fsum(v['purity_sum'] for v in values)
                    base=reference['summary']['all']['methods']['lexical_average'][scope]
                    if pairs!=base['pairs']:raise ValueError('unchanged original pair support required')
                    entry[scope]={'pairs':pairs,'purity_sum':weight,'purity':weight/pairs,'loss_vs_offline_tfidf_average':base['purity']-weight/pairs}
                summary[policy][kind][order]=entry
    return {'schema_version':1,'scope':'fixed_online_lexical_rotation_quality_control_not_production_selection','support':dict(support),
        'source_sha256':source,'source_capture_sha256':source_capture,'preflight_sha256':PREFLIGHT,'geometry_sha256':GEOMETRY,'macro_reference_sha256':REFERENCE,
        'summary':summary,'cosine_distortions':distortions,'records':records,
        'protocol':{'new_inference':0,'llm_calls':0,'heldout_read':False,'full_ingestion_gate':False,'production_selection':False,'author_GRINCH':False,'label_cut_uses_gold':False},
        'code_pins':{n:digest(Path(__file__).with_name(n)) for n in ['qmsum_stable_frame_quality.py','stable_frame_quality.py','STABLE_FRAME_QUALITY_PROTOCOL.md','stable_lexical_frame.py',
            'STABLE_LEXICAL_PROTOCOL.md','sparse_average_tree.py','qmsum_sparse_average.py','online_tree.py','data.py','episode_blocks.py','block_hierarchy.py','captured_hierarchy.py',
            'qmsum_hierarchy.py','qmsum.py','topic_labels.py','subject_labels.py','label_noise.py','uv.lock']},
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
    print(json.dumps({'support':result['support'],'summary':result['summary']}),flush=True)


if __name__=='__main__':main()
