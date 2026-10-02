"""Independent captured topology/source-pair/label audit; no topic helper imports."""
import argparse
from collections import Counter
import hashlib
import json
import math
from pathlib import Path
import numpy as np
from scipy import sparse
from sklearn.feature_extraction.text import ENGLISH_STOP_WORDS


def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()


def audit(root,path):
    data=json.loads(path.read_text());pre=root/'topic-qmsum-stable-frame-preflight-first-20261002'
    for name,pin in data['code_pins'].items():assert sha(Path(__file__).with_name(name))==pin
    assert sha(pre/'results.json')==data['preflight_sha256'];preflight=json.loads((pre/'results.json').read_text())
    g=root/'topic-qmsum-block-geometry-first-20261002/results.json';assert sha(g)==data['geometry_sha256'];geometry=json.loads(g.read_text())
    source=root/'topic-qmsum-author-training-20261002/data/ALL/jsonl/train.jsonl';assert sha(source)==data['source_sha256']
    meetings=[json.loads(line) for line in source.read_bytes().splitlines()]
    ref=root/'topic-qmsum-block-hierarchy-final-first-20261002.json';assert sha(ref)==data['macro_reference_sha256'];reference=json.loads(ref.read_text())
    c=root/'topic-qmsum-label-cleanup-first-20261002';assert sha(c/'results.json')=='064ff3731955b515a07e12e3897102e2cd9d137ac91c43ec222e393a2e492d08'
    ledger=json.loads((c/'results.json').read_text());schema=ledger['matrices']['exact_markers']
    for name,pin in schema['files'].items():assert sha(c/name)==pin
    source_counts=sparse.csr_matrix(tuple(np.load(c/f'exact_markers-{k}.npy') for k in ['data','indices','indptr']),shape=tuple(schema['shape']))
    v=root/'topic-qmsum-lexical-context-final-first-20261002';original=json.loads((v/'results.json').read_text())
    assert sha(v/'results.json')==ledger['baseline_sha256'];assert sha(v/'vocabulary.json')==original['vocabulary_sha256'];terms=json.loads((v/'vocabulary.json').read_text())
    del ledger,original
    excluded=frozenset(ENGLISH_STOP_WORDS)|{'yeah','okay','ok','uh','um','erm','hmm','mm','oh'}
    banks={}
    for policy in ['marker_only_tf','static_noise_filtered_tf']:
        for name,pin in preflight['files'].items():assert sha(pre/name)==pin
        banks[policy]=sparse.csr_matrix(tuple(np.load(pre/f'{policy}-{k}.npy') for k in ['data','indices','indptr']),shape=(8196,16384))
    cases=pairs_checked=labels_checked=representatives_checked=0;maximum_error=0.;reduction_ties=[]
    def close(a,b):
        nonlocal maximum_error
        if isinstance(a,dict):
            assert a.keys()==b.keys()
            for k in a:close(a[k],b[k])
        elif isinstance(a,list):
            assert len(a)==len(b)
            for x,y in zip(a,b):close(x,y)
        elif isinstance(a,float):
            maximum_error=max(maximum_error,abs(a-b));assert math.isclose(a,b,rel_tol=1e-12,abs_tol=1e-8),(a,b)
        else:assert a==b,(a,b)
    for m,(meeting,original) in enumerate(zip(meetings,geometry['records'])):
        blocks=original['blocks'];n=len(blocks);size=len(meeting['meeting_transcripts']);owners=np.full(size,-1,dtype=np.int64)
        for b in blocks:owners[b['start']:b['end']+1]=b['ordinal']
        members=[[] for _ in range(size)]
        for topic,row in enumerate(meeting['topic_list']):
            spans=sorted((int(a),int(b)) for a,b in row['relevant_text_span']);assert all(a[1]+1<b[0] for a,b in zip(spans,spans[1:]))
            for component,(a,b) in enumerate(spans):
                for i in range(a,b+1):members[i].append((topic,component))
        known=np.array([i for i in range(size) if owners[i]>=0 and len(members[i])==1]);assert known.tolist()==original['known_source_ordinals']
        gold=np.array([members[i][0][0] for i in known]);parts=np.array([members[i][0][1] for i in known]);owned=owners[known]
        leaf_counts=np.zeros((n,len(meeting['topic_list'])),dtype=np.int64)
        for b,topic in zip(owned,gold):leaf_counts[b,topic]+=1
        counts=sparse.vstack([sparse.csr_matrix(source_counts[original['source_offset']+b['start']:original['source_offset']+b['end']+1].sum(axis=0)) for b in blocks],format='csr')
        # Exact winner replay also needs the frozen CSR incidence-layout path:
        # equal integer counts in a different sparse index order can change a
        # normalized TF value by one ULP. Independently validate count values.
        source_rows=np.flatnonzero(owners>=0)
        incidence=sparse.csr_matrix((np.ones(len(source_rows)),(owners[source_rows],source_rows)),shape=(n,size))
        replay_counts=(incidence@source_counts[original['source_offset']:original['source_offset']+size]).tocsr()
        assert (replay_counts-counts).nnz==0
        rows=[r for r in data['records'] if r['meeting']==m];assert len(rows)==8
        for row in rows:
            used=replay_counts.copy().astype(float);states=[]
            if row['policy']=='static_noise_filtered_tf':used=used.multiply([t not in excluded for t in terms]).tocsr();used.eliminate_zeros()
            for i in range(n):
                states.append('ready' if used[i].nnz else 'filtered_empty_fallback' if counts[i].nnz else 'empty')
            fallback=np.array([s=='filtered_empty_fallback' for s in states]);used+=sparse.diags(fallback.astype(float))@replay_counts
            used.data=1+np.log(used.data);norms=np.sqrt(np.asarray(used.multiply(used).sum(axis=1)).ravel());identity=(sparse.diags(1/norms)@used).tocsr()
            lo=original['block_directions_offset'];features=banks[row['policy']][lo:lo+n] if row['kind']=='hashed' else identity
            if row['kind']=='hashed' and row['order']=='forward':
                errors=((features@features.T).toarray()-(identity@identity.T).toarray())[np.triu_indices(n,1)]
                close({'meeting':m,'policy':row['policy'],'pairs':len(errors),'signed_sum':math.fsum(errors),
                    'absolute_sum':math.fsum(abs(errors)),'maximum_absolute':float(max(abs(errors),default=0)),
                    'minimum_signed':float(min(errors,default=0)),'maximum_signed':float(max(errors,default=0))},
                    next(v for v in data['cosine_distortions'] if (v['meeting'],v['policy'])==(m,row['policy'])))
            assert states==row['states'];tree=row['tree'];children={int(k):v for k,v in tree['children'].items()};leaves={int(k):v for k,v in tree['leaves'].items()}
            order=list(range(n));order=order[::-1] if row['order']=='reverse' else order
            assert row['state']['order']==order and row['state']['identities']==list(map(str,order));assert sorted(leaves.values())==list(range(n))
            descendants={k:[v] for k,v in leaves.items()};node_counts={k:leaf_counts[v].copy() for k,v in leaves.items()};lcas=np.full((n,n),-1,dtype=np.int64)
            post=[];stack=[(tree['root'],False)];seen=set()
            while stack:
                node,expanded=stack.pop()
                if expanded:post.append(node);continue
                assert node not in seen;seen.add(node);stack.append((node,True))
                if node in children:stack.extend((c,False) for c in children[node])
            assert seen==set(children)|set(leaves)
            for node in [v for v in post if v in children]:
                a,b=children[node];left,right=descendants[a],descendants[b];assert not set(left)&set(right)
                lcas[np.ix_(left,right)]=node;lcas[np.ix_(right,left)]=node;descendants[node]=sorted(left+right);node_counts[node]=node_counts[a]+node_counts[b]
            assert descendants[tree['root']]==list(range(n));total=node_counts[tree['root']];metrics={}
            for scope in ['all','far','crossspan']:
                topics=[]
                for topic in range(len(meeting['topic_list'])):
                    ids=np.flatnonzero(gold==topic);i,j=np.triu_indices(len(ids),1);a,b=ids[i],ids[j];valid=owned[a]!=owned[b]
                    if scope!='all':valid&=known[b]-known[a]>4
                    if scope=='crossspan':valid&=parts[a]!=parts[b]
                    nodes=lcas[owned[a[valid]],owned[b[valid]]];assert np.all(nodes>=0);unique,pop=np.unique(nodes,return_counts=True)
                    pairs=len(nodes);weight=math.fsum(int(p)*int(node_counts[int(k)][topic])/int(node_counts[int(k)].sum()) for k,p in zip(unique,pop))
                    topics.append({'topic':topic,'known_rows':int(total[topic]),'pairs':pairs,'purity_sum':weight,'purity':weight/pairs if pairs else None,
                        'lca_known_rows_sum':sum(int(p)*int(node_counts[int(k)].sum()) for k,p in zip(unique,pop)),'root_lca_pairs':int((nodes==tree['root']).sum())});pairs_checked+=pairs
                pairs=sum(t['pairs'] for t in topics);weight=math.fsum(t['purity_sum'] for t in topics);root_weight=math.fsum(t['pairs']*int(total[t['topic']])/len(known) for t in topics)
                metrics[scope]={'pairs':pairs,'purity_sum':weight,'purity':weight/pairs if pairs else None,'root_only_purity_sum':root_weight,
                    'root_only_purity':root_weight/pairs if pairs else None,'lca_known_rows_sum':sum(t['lca_known_rows_sum'] for t in topics),'root_lca_pairs':sum(t['root_lca_pairs'] for t in topics),'topics':topics}
            close(metrics,row['metrics'])
            cut=[tree['root']]
            while len(cut)<min(8,n):
                node=min([v for v in cut if v in children],key=lambda v:(-len(descendants[v]),descendants[v]));cut.remove(node);cut.extend(children[node])
            groups=[descendants[v] for v in sorted(cut,key=lambda v:descendants[v])];assert groups==[v['blocks'] for v in row['display_labels']]
            eligible=np.array([s=='ready' for s in states]);clean=(sparse.diags(eligible.astype(float))@counts.multiply([t not in excluded for t in terms])).tocsr();clean.eliminate_zeros()
            aggregates=[np.asarray(clean[g].sum(axis=0)).ravel() for g in groups];lengths=[float(v.sum()) for v in aggregates];frequency=np.sum(aggregates,axis=0);average=sum(lengths)/len(lengths)
            for g,c,l,label in zip(groups,aggregates,lengths,row['display_labels']):
                ids=np.flatnonzero(c>0);scored=[{'term':terms[j],'score':float(c[j]/max(l,1)*math.log1p(average/frequency[j]))} for j in ids]
                wanted=sorted(scored,key=lambda v:(-v['score'],v['term']))[:8];close(wanted,label['labels']);labels_checked+=len(wanted)
                assert label['label_eligible_blocks']==[i for i in g if eligible[i]]
                # Scalar pairwise average is independent of mean-vector helper.
                cosines=(features[g]@features[g].T).toarray();means=[math.fsum(v)/len(g) for v in cosines]
                pairwise_representative=min(range(len(g)),key=lambda j:(-means[j],g[j]))
                # Frozen winner uses dot with computed mean, not a reassociated
                # pairwise reduction. Check that operation exactly, and validate
                # its scores/max against independent pairwise algebra separately.
                replay=np.asarray((features[g]@sparse.csr_matrix(features[g].mean(axis=0)).T).toarray()).ravel()
                representative=min(range(len(g)),key=lambda j:(-replay[j],g[j]))
                assert g[representative]==label['representative_block'],(m,row['policy'],row['kind'],row['order'],g,g[representative],label['representative_block'],max(replay)-replay[g.index(label['representative_block'])])
                for a,b in zip(means,replay):assert abs(a-b)<=1e-12
                gap=max(means)-means[representative];assert gap<=1e-12
                if representative!=pairwise_representative:
                    reduction_ties.append({'meeting':m,'policy':row['policy'],'kind':row['kind'],'order':row['order'],
                        'blocks':g,'mean_vector_winner':g[representative],'pairwise_winner':g[pairwise_representative],'pairwise_score_gap':gap})
                close(means[representative],label['representative_mean_similarity']);b=blocks[g[representative]]
                assert label['representative_source']=={'start':b['start'],'end':b['end'],'text':'\n'.join(t['content'] for t in meeting['meeting_transcripts'][b['start']:b['end']+1])};representatives_checked+=1
            cases+=1
        if m%20==0:print(json.dumps({'audited_meetings':m+1}),flush=True)
    assert cases==1296 and len(data['records'])==cases
    for policy,kinds in data['summary'].items():
        for kind,orders in kinds.items():
            for order,scopes in orders.items():
                rows=[r for r in data['records'] if (r['policy'],r['kind'],r['order'])==(policy,kind,order)]
                for scope,entry in scopes.items():
                    pairs=sum(r['metrics'][scope]['pairs'] for r in rows);weight=math.fsum(r['metrics'][scope]['purity_sum'] for r in rows)
                    close({'pairs':pairs,'purity_sum':weight,'purity':weight/pairs,
                        'loss_vs_offline_tfidf_average':reference['summary']['all']['methods']['lexical_average'][scope]['purity']-weight/pairs},entry)
    return {'verified':True,'capture_sha256':sha(path),'auditor_sha256':sha(Path(__file__)),'tree_cases':cases,'pair_scope_contributions':pairs_checked,
        'label_terms_checked':labels_checked,'representatives_checked':representatives_checked,'maximum_numeric_error':maximum_error,
        'representative_reduction_ties':reduction_ties,
        'limitations':['shared frozen count/encoded frame banks, NumPy/SciPy and sklearn static stoplist',
            'independent pair and label calculation; exact representative IDs replay shared frozen CSR incidence layout and mean-vector operation with separate source-count/pairwise score/max audit',
            'not independent tokenizer/hash audit or rotation algorithm replay',
            'topology evidence is not semantic label/multitag/ancestor, global lifecycle, full-ingestion or Rust parity evidence']}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--artifacts',type=Path,required=True);p.add_argument('--capture',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    result=audit(a.artifacts,a.capture)
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps({k:v for k,v in result.items() if k!='representative_reduction_ties'}|{'representative_reduction_tie_count':len(result['representative_reduction_ties'])}),flush=True)


if __name__=='__main__':main()
