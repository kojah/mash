"""Scalar original-count lexical features and source/LCA/global linkage audit."""
import argparse
from collections import Counter
import hashlib
import json
import math
from pathlib import Path
import numpy as np
from scipy import sparse
from source_lca_audit import record


def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()


def audit(root,path):
    pins={n:sha(Path(__file__).with_name(n)) for n in ['audit_lexical_granularity.py','source_lca_audit.py','linkage_minimum.py']}
    data=json.loads(path.read_text());gp=root/'topic-qmsum-block-geometry-first-20261002/results.json';assert sha(gp)==data['geometry_sha256'];geometry=json.loads(gp.read_text())
    dp=root/'topic-qmsum-unit-granularity-first-20261002.json';assert sha(dp)==data['dense_sha256'];dense=json.loads(dp.read_text())
    source=root/'topic-qmsum-author-training-20261002/data/ALL/jsonl/train.jsonl';assert sha(source)==data['source_sha256'];meetings=[json.loads(x) for x in source.read_bytes().splitlines()]
    for n,pin in data['code_pins'].items():assert sha(Path(__file__).with_name(n))==pin
    lr=root/'topic-qmsum-label-cleanup-first-20261002';lp=lr/'results.json';assert sha(lp)=='064ff3731955b515a07e12e3897102e2cd9d137ac91c43ec222e393a2e492d08'
    ledger=json.loads(lp.read_text());schema=ledger['matrices']['exact_markers']
    for n,pin in schema['files'].items():assert sha(lr/n)==pin
    counts=sparse.csr_matrix(tuple(np.load(lr/f'exact_markers-{k}.npy') for k in ['data','indices','indptr']),shape=tuple(schema['shape']))
    br=root/'topic-qmsum-lexical-context-final-first-20261002';bp=br/'results.json';assert sha(bp)==ledger['baseline_sha256'];baseline=json.loads(bp.read_text())
    assert sha(br/'idf.npy')==baseline['idf_sha256'] and sha(br/'vocabulary.json')==baseline['vocabulary_sha256'];idf=np.load(br/'idf.npy');terms=json.loads((br/'vocabulary.json').read_text());assert len(terms)==counts.shape[1]
    del ledger,baseline
    recomputed={};zero_counts=Counter();nnz_counts=Counter();coordinates=trees=merges=pairs=0;maximum=global_gap=0.
    for ordinal,(meeting,g) in enumerate(zip(meetings,geometry['records'])):
        size=len(meeting['meeting_transcripts']);offset=g['source_offset'];owners=np.full(size,-1,dtype=np.int64)
        for b in g['blocks']:owners[b['start']:b['end']+1]=b['ordinal']
        positions=np.flatnonzero(owners>=0).tolist();local=counts[offset:offset+size];original=dense['context_lineage'][ordinal]['units']
        memberships=[[] for _ in range(size)]
        for topic,entry in enumerate(meeting['topic_list']):
            spans=sorted((int(a),int(b)) for a,b in entry['relevant_text_span']);assert all(a[1]+1<b[0] for a,b in zip(spans,spans[1:]))
            for component,(a,b) in enumerate(spans):
                for i in range(a,b+1):memberships[i].append((topic,component))
        def words(start,end):
            result=Counter()
            for i in range(start,end+1):
                r=local[i]
                for j,v in zip(r.indices,r.data):assert v>0 and v==int(v);result[int(j)]+=int(v)
            return result
        chosen=[r for r in data['records'] if r['meeting']==ordinal];assert len(chosen)==3
        for r in chosen:
            name=r['representation'];assert r['source_ordinals']==positions and r['original_physical_owners']==owners[positions].tolist()
            if name=='raw_message':bank=[words(i,i) for i in positions];expected_owners=list(range(len(positions)))
            elif name=='causal_context':
                bank=[]
                for i in positions:
                    u=original[i];assert u['ordinal']==i and u['status']=='ready' and 0<=i-u['start']<=4 and u['tokens']<=512
                    assert all(original[j]['status']=='ready' for j in range(u['start'],i+1));bank.append(words(u['start'],i))
                expected_owners=list(range(len(positions)))
            else:
                assert name=='physical_block';bank=[words(b['start'],b['end']) for b in g['blocks']];expected_owners=owners[positions].tolist()
            assert r['discovery_owners']==expected_owners
            rows=[];columns=[];values=[]
            for i,word_counts in enumerate(bank):
                weighted={j:(1+math.log(c))*float(idf[j]) for j,c in sorted(word_counts.items())};norm=math.sqrt(math.fsum(v*v for v in weighted.values()))
                if norm:
                    for j,v in weighted.items():rows.append(i);columns.append(j);values.append(v/norm);coordinates+=1
            v=sparse.csr_matrix((values,(rows,columns)),shape=(len(bank),len(terms)));zeros=sum(not b for b in bank)
            assert zeros==r['zero_lexical_units'] and v.nnz==r['feature_nnz'];zero_counts[name]+=zeros;nnz_counts[name]+=v.nnz
            distance=1-np.clip((v@v.T).toarray(),-1,1);np.fill_diagonal(distance,0)
            verified=record(r,memberships,distance);recomputed[ordinal,name]=verified['metrics'];trees+=1;merges+=len(r['merges']);pairs+=verified['positive_pair_scope_contributions']
            maximum=max(maximum,verified['maximum_numeric_error']);global_gap=max(global_gap,verified['global_minimum']['maximum_chosen_minus_global_minimum'])
        if ordinal%20==0:print(json.dumps({'audited_meetings':ordinal+1}),flush=True)
    for name,s in data['summary'].items():
        assert s['zero_lexical_units']==zero_counts[name] and s['feature_nnz']==nnz_counts[name]
        for scope,m in s['scopes'].items():
            values=[recomputed[i,name][scope] for i in range(162)];p=sum(x['pairs'] for x in values);weight=math.fsum(x['purity_sum'] for x in values)
            assert p==m['pairs'] and m['dense_reference']==dense['summary'][name][scope]['purity']
            assert math.isclose(weight,m['purity_sum'],rel_tol=1e-12,abs_tol=1e-12) and math.isclose(weight/p,m['purity'],rel_tol=1e-12,abs_tol=1e-12)
    assert trees==486 and pins=={n:sha(Path(__file__).with_name(n)) for n in pins}
    return {'capture_sha256':sha(path),'code_pins':pins,'trees':trees,'merges':merges,'scalar_coordinates':coordinates,'positive_pair_scope_contributions':pairs,
        'maximum_numeric_error':maximum,'maximum_chosen_minus_global_minimum':global_gap,'zero_lexical_units':dict(zero_counts),
        'limitations':['shared pinned original count ledger/IDF/vocabulary and NumPy/SciPy sparse Gram; source tokenizer and dense context boundaries validated previously',
            'independent source word pooling, scalar TF/IDF/norm, original annotations/LCA/pairs, global average minima and summary; ties allowed within frozen1e-12',
            'not independent label/multitag/semantic ancestry/guided-search or online lifecycle/full-ingestion/selected Rust parity evidence']}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--artifacts',type=Path,required=True);p.add_argument('--capture',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    result=audit(a.artifacts,a.capture)
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps(result),flush=True)


if __name__=='__main__':main()
