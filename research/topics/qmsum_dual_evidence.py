"""Frozen original-plus-context QMSum representation comparison; no new inference."""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import resource
import time

import numpy as np
from scipy import sparse

from causal_lexical_context import rank
from dual_lexical_evidence import fuse
from external_context import compare
from qmsum import load, membership
from qmsum_lexical_context import paired, summarize


BASELINE = 'e3c89677f538f65f3e23a5a7de14de6ef9340febb40a408d97ebb275cb11b488'


def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()


def experiment(a):
    start=time.perf_counter();cpu=time.process_time();path=a.baseline/'results.json'
    if digest(path)!=BASELINE:raise ValueError('audited complete original/context baseline required')
    original=json.loads(path.read_text());capture,source,meetings=load(a.source)
    if (capture,source)!=(original['capture_sha256'],original['source_sha256']):raise ValueError('source drift')
    for name,pin in original['code_pins'].items():
        if digest(Path(__file__).with_name(name))!=pin:raise ValueError('baseline code drift')
    matrices={}
    for name in ['current','context']:
        description=original['matrices'][name]
        for filename,pin in description['files'].items():
            if digest(a.baseline/filename)!=pin:raise ValueError('baseline matrix drift')
        arrays=[np.load(a.baseline/f'{name}-{key}.npy',allow_pickle=False) for key in ['data','indices','indptr']]
        matrices[name]=sparse.csr_matrix(tuple(arrays),shape=tuple(description['shape']))
    setup_ms=(time.perf_counter()-start)*1000;begin=time.perf_counter();offset=0;queries=[]
    old={(r['meeting'],r['ordinal']):r for r in original['queries']};seen=set()
    for meeting in meetings:
        size=len(meeting.turns);members,_=membership(meeting)
        labels=np.array([s[0] if len(s)==1 else -1 for s in members]);identities=np.arange(size)
        nonempty=(matrices['current'][offset:offset+size].getnnz(axis=1)>0)&(matrices['context'][offset:offset+size].getnnz(axis=1)>0)
        selected=[r for r in original['queries'] if r['meeting']==meeting.ordinal]
        for block_start in range(0,len(selected),128):
            rows=selected[block_start:block_start+128];positions=np.array([r['ordinal'] for r in rows])
            scores={name:(matrices['current'][offset+positions] @ matrices[name][offset:offset+size].T).toarray() for name in matrices}
            for row,r in enumerate(rows):
                seen.add((r['meeting'],r['ordinal']));record={k:v for k,v in r.items() if k!='policies'}
                record['policies']=dict(r['policies'])
                if r['unsupported'] is None:
                    mask=(labels>=0)&nonempty&(np.abs(identities-r['ordinal'])>4)
                    candidates=identities[mask];relevant=labels[mask]==labels[r['ordinal']]
                    if len(candidates)!=r['candidates'] or int(relevant.sum())!=r['relevant']:raise ValueError('support drift')
                    for name in matrices:
                        if rank(scores[name][row,mask],candidates,relevant)!=r['policies'][name]:raise ValueError('baseline rank drift')
                    merged=fuse(scores['current'][row,mask],scores['context'][row,mask],candidates)
                    record['policies']['dual']=rank(merged,candidates,relevant)
                queries.append(record)
        offset+=size
        if meeting.ordinal%25==0:print(json.dumps({'processed_meetings':meeting.ordinal+1}),flush=True)
    if seen!=set(old) or len(queries)!=len(old):raise ValueError('incomplete source query coverage')
    supported=[r for r in queries if r['unsupported'] is None]
    summary={name:summarize([r['policies'][name] for r in supported]) for name in ['current','context','dual']}
    comparisons={}
    for baseline in ['current','context']:
        direction={key:dict(Counter('improved' if r['policies']['dual'][key]>r['policies'][baseline][key]
                                   else 'worse' if r['policies']['dual'][key]<r['policies'][baseline][key] else 'tied'
                                   for r in supported)) for key in ['mrr','hit1','hit10','recall1','recall10']}
        adapted=[{'meeting':r['meeting'],'policies':{'current':r['policies'][baseline],'context':r['policies']['dual']}} for r in supported]
        comparisons[baseline]={'query_directions':direction,'paired_mrr':paired(adapted)}
    result={'schema_version':1,'scope':'in_sample_dual_view_representation_not_topic_selection',
            'baseline_sha256':BASELINE,'source_sha256':source,'capture_sha256':capture,
            'protocol':{'method':'equal_weight_full_list_rrf','k':60,'rank_base':1,'tie':'score_descending_then_original_ordinal',
                        'identity':'one_candidate_per_original_source_turn','pre_fusion_truncation':False,
                        'support':'unchanged_audited_baseline','new_inference_calls':0,'held_out_sources_read':False},
            'support':original['support'],'unsupported_queries':original['unsupported_queries'],'summary':summary,
            'comparisons':comparisons,'queries':queries,
            'code_pins':{name:digest(Path(__file__).with_name(name)) for name in
                         ['qmsum_dual_evidence.py','dual_lexical_evidence.py','causal_lexical_context.py','qmsum.py','qmsum_lexical_context.py','uv.lock']}}
    result['timings']={'setup_ms':setup_ms,'evaluation_ms':(time.perf_counter()-begin)*1000,
                       'total_after_imports_ms':(time.perf_counter()-start)*1000,'cpu_ms':(time.process_time()-cpu)*1000,
                       'peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}
    return result


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for key in ['source','baseline','output-directory']:p.add_argument('--'+key,type=Path,required=True)
    p.add_argument('--compare',type=Path);a=p.parse_args()
    if a.output_directory.exists() or a.output_directory.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    a.output_directory.mkdir(parents=True);r=experiment(a)
    if a.compare:compare(json.loads(a.compare.read_text()),r)
    with (a.output_directory/'results.json').open('x') as f:json.dump(r,f,indent=2,allow_nan=False)
    print(json.dumps({k:r[k] for k in ['summary','comparisons','timings']}),flush=True)


if __name__=='__main__':main()
