"""Original-source LCA and captured merge-height audit, without shared metrics."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import numpy as np


def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()


def audit(root,path):
    data=json.loads(path.read_text());gp=root/'topic-qmsum-block-geometry-first-20261002/results.json'
    assert sha(gp)==data['geometry_sha256'];geometry=json.loads(gp.read_text())
    source=root/'topic-qmsum-author-training-20261002/data/ALL/jsonl/train.jsonl';assert sha(source)==data['source_sha256'];meetings=[json.loads(x) for x in source.read_bytes().splitlines()]
    for n,pin in data['code_pins'].items():assert sha(Path(__file__).with_name(n))==pin
    vr=root/'topic-qmsum-semantic-vectors-first-20261002';vp=vr/'results.json'
    assert sha(vp)=='05642c1a77dab33d2940c39979b0ac4e4b420420debcb30ca529d7bad99bdf98';vmeta=json.loads(vp.read_text())
    assert sha(vr/'raw-vectors.npy')==vmeta['files']['raw-vectors.npy']['sha256'];raw=np.load(vr/'raw-vectors.npy',mmap_mode='r',allow_pickle=False)
    ready=np.array([all(v['status']=='ready' for v in r['variants'].values()) for r in vmeta['rows']]);tokens=np.array([r['variants']['raw']['tokens'] for r in vmeta['rows']]);del vmeta
    pp=gp.parent/'block-directions.npy';assert sha(pp)==data['block_vectors_sha256'];pooled=np.load(pp,mmap_mode='r',allow_pickle=False)
    maximum=0.;trees=merge_checks=pair_checks=0;recomputed={}
    def close(a,b):
        nonlocal maximum
        if a is None:assert b is None;return
        maximum=max(maximum,abs(a-b));assert math.isclose(a,b,rel_tol=1e-12,abs_tol=1e-12),(a,b)
    for ordinal,(meeting,g) in enumerate(zip(meetings,geometry['records'])):
        size=len(meeting['meeting_transcripts']);offset=g['source_offset'];owner=np.full(size,-1,dtype=np.int64)
        for b in g['blocks']:owner[b['start']:b['end']+1]=b['ordinal']
        assert np.array_equal(owner>=0,ready[offset:offset+size]);positions=np.flatnonzero(owner>=0).tolist();physical=owner[positions]
        membership=[[] for _ in range(size)]
        for topic,entry in enumerate(meeting['topic_list']):
            spans=sorted((int(a),int(b)) for a,b in entry['relevant_text_span']);assert all(a[1]+1<b[0] for a,b in zip(spans,spans[1:]))
            for component,(a,b) in enumerate(spans):
                for i in range(a,b+1):membership[i].append((topic,component))
        known=np.array([j for j,i in enumerate(positions) if len(membership[i])==1]);gold=np.array([membership[positions[j]][0][0] for j in known]);parts=np.array([membership[positions[j]][0][1] for j in known]);original=np.array(positions)[known]
        context=np.zeros((size,384));units=[]
        for end in range(size):
            if not ready[offset+end]:units.append({'ordinal':end,'start':None,'tokens':0,'status':'unsupported_source','limit':'unsupported_current_source'});continue
            start=end;total=int(tokens[offset+end]);limit=None
            while start>0 and end-start<4:
                if not ready[offset+start-1]:limit='unsupported_prior_source';break
                if total+int(tokens[offset+start-1])>512:limit='whole_turn_token_limit';break
                start-=1;total+=int(tokens[offset+start])
            limit=limit or ('meeting_root' if start==0 else 'five_turn_limit')
            summed=np.sum(raw[offset+start:offset+end+1],axis=0,dtype=np.float64);norm=float(np.linalg.norm(summed));assert norm>1e-12;context[end]=summed/norm
            units.append({'ordinal':end,'start':start,'tokens':total,'status':'ready','limit':limit})
        assert data['context_lineage'][ordinal]=={'meeting':ordinal,'source_offset':offset,'units':units}
        lo=g['block_directions_offset'];chosen=[r for r in data['records'] if r['meeting']==ordinal];assert len(chosen)==3
        for r in chosen:
            assert r['source_ordinals']==positions and r['original_physical_owners']==physical.tolist()
            name=r['representation'];vectors={'raw_message':raw[offset:offset+size][positions],'causal_context':context[positions],'physical_block':pooled[lo:lo+len(g['blocks'])]}[name]
            owners=physical if name=='physical_block' else np.arange(len(positions));assert r['discovery_owners']==owners.tolist()
            n=len(vectors);tree=r['tree'];children={int(k):v for k,v in tree['children'].items()};leaves={int(k):v for k,v in tree['leaves'].items()}
            assert leaves=={i:i for i in range(n)} and tree['root']==2*n-2;gram=vectors@vectors.T;distance=1-np.clip(gram,-1,1);np.fill_diagonal(distance,0)
            leaf_members={i:[i] for i in range(n)};counts=np.zeros((2*n-1,len(meeting['topic_list'])),dtype=np.int64);lca=np.full((n,n),-1,dtype=np.int64)
            for j,topic in zip(known,gold):counts[owners[j],topic]+=1
            for k,merge in enumerate(r['merges']):
                node=n+k;a,b=int(merge[0]),int(merge[1]);assert children[node]==[a,b] and a<node and b<node
                left,right=leaf_members[a],leaf_members[b];assert not set(left)&set(right)
                close(float(distance[np.ix_(left,right)].mean()),merge[2]);assert len(left)+len(right)==merge[3]
                lca[np.ix_(left,right)]=node;lca[np.ix_(right,left)]=node;leaf_members[node]=left+right;counts[node]=counts[a]+counts[b];merge_checks+=1
            assert sorted(leaf_members[tree['root']])==list(range(n));totals=counts.sum(axis=1);metrics={}
            for scope in ['all','far','crossspan']:
                topic_rows=[]
                for topic in sorted(set(gold)):
                    ids=np.flatnonzero(gold==topic);a,b=np.triu_indices(len(ids),1);a,b=ids[a],ids[b];mask=physical[known[a]]!=physical[known[b]]
                    if scope!='all':mask&=original[b]-original[a]>4
                    if scope=='crossspan':mask&=parts[a]!=parts[b]
                    nodes=lca[owners[known[a[mask]]],owners[known[b[mask]]]];assert (nodes>=0).all()
                    weight=math.fsum(float(x) for x in counts[nodes,topic]/totals[nodes]);pairs=len(nodes);pair_checks+=pairs
                    topic_rows.append({'topic':int(topic),'pairs':pairs,'purity_sum':weight})
                pairs=sum(x['pairs'] for x in topic_rows);weight=math.fsum(x['purity_sum'] for x in topic_rows);m={'pairs':pairs,'purity_sum':weight,'purity':weight/pairs if pairs else None,'topics':topic_rows};captured=r['metrics'][scope]
                assert m['pairs']==captured['pairs'] and len(topic_rows)==len(captured['topics'])
                close(weight,captured['purity_sum']);close(m['purity'],captured['purity'])
                for x,y in zip(topic_rows,captured['topics']):assert x['topic']==y['topic'] and x['pairs']==y['pairs'];close(x['purity_sum'],y['purity_sum'])
                metrics[scope]=m
            recomputed[ordinal,name]=metrics;trees+=1
        if ordinal%20==0:print(json.dumps({'audited_meetings':ordinal+1}),flush=True)
    for name,scopes in data['summary'].items():
        for scope,m in scopes.items():
            rows=[recomputed[i,name][scope] for i in range(162)];pairs=sum(x['pairs'] for x in rows);weight=math.fsum(x['purity_sum'] for x in rows)
            assert pairs==m['pairs'];close(weight,m['purity_sum']);close(weight/pairs,m['purity'])
    assert trees==486
    return {'capture_sha256':sha(path),'auditor_sha256':sha(Path(__file__)),'trees':trees,'merge_heights_checked':merge_checks,
        'positive_pair_scope_contributions':pair_checks,'maximum_numeric_error':maximum,
        'limitations':['shared frozen vectors/NumPy dot, norm and mean operations; context source-boundary selection reconstructed independently',
            'verifies source annotations, LCA pairs, merge mean distances/populations and summaries, not global-minimum/tie choice of each linkage merge',
            'no label/cut/multitag/semantic ancestry/guided-search or online lifecycle/full-ingestion/Rust parity proof']}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--artifacts',type=Path,required=True);p.add_argument('--capture',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    result=audit(a.artifacts,a.capture)
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps(result),flush=True)


if __name__=='__main__':main()
