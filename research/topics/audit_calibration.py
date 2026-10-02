"""Independent source/model/weighted geometry and native-pair audit."""
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
def pin(v):return hashlib.sha256(json.dumps(v,sort_keys=True,ensure_ascii=False,separators=(',',':'),allow_nan=False).encode()).hexdigest()


def audit(root,capture):
    data=json.loads((capture/'results.json').read_text())
    pre=root/'topic-qmsum-stable-frame-preflight-first-20261002/results.json'
    assert sha(pre)=='1c989b0d905270a3b667e6a62c4f12e73ef8cce50e4a075a47a55675731ead86';preflight=json.loads(pre.read_text())
    for name,wanted in data['code_pins'].items():assert sha(Path(__file__).with_name(name))==wanted
    for name,wanted in data['files'].items():assert sha(capture/name)==wanted
    g=root/'topic-qmsum-block-geometry-first-20261002/results.json';assert sha(g)==data['geometry_sha256'];geometry=json.loads(g.read_text())
    source=root/'topic-qmsum-author-training-20261002/data/ALL/jsonl/train.jsonl';assert sha(source)==data['source_sha256'];meetings=[json.loads(line) for line in source.read_bytes().splitlines()]
    ref=root/'topic-qmsum-block-hierarchy-final-first-20261002.json';tf=root/'topic-qmsum-stable-frame-quality-first-20261002.json'
    assert sha(ref)==data['offline_reference_sha256'] and sha(tf)==data['tf_reference_sha256']
    baseline_trees={}
    for path in [ref,tf]:
        old=json.loads(path.read_text())
        for r in old['records']:
            if path==ref and r['method']!='lexical_average':continue
            key=(r['meeting'],'offline_tfidf','offline_average','offline') if path==ref else (r['meeting'],r['policy'],r['kind'],r['order'])
            baseline_trees[key]=r['tree']
        del old
    c=root/'topic-qmsum-label-cleanup-first-20261002';assert sha(c/'results.json')=='064ff3731955b515a07e12e3897102e2cd9d137ac91c43ec222e393a2e492d08'
    ledger=json.loads((c/'results.json').read_text());schema=ledger['matrices']['exact_markers']
    for name,wanted in schema['files'].items():assert sha(c/name)==wanted
    counts=sparse.csr_matrix(tuple(np.load(c/f'exact_markers-{k}.npy') for k in ['data','indices','indptr']),shape=tuple(schema['shape']))
    v=root/'topic-qmsum-lexical-context-final-first-20261002';old=json.loads((v/'results.json').read_text());assert sha(v/'results.json')==ledger['baseline_sha256'];assert sha(v/'vocabulary.json')==old['vocabulary_sha256'];terms=json.loads((v/'vocabulary.json').read_text());del ledger,old
    excluded=frozenset(ENGLISH_STOP_WORDS)|{'yeah','okay','ok','uh','um','erm','hmm','mm','oh'};blocks=[]
    for row in geometry['records']:
        for b in row['blocks']:
            r=sparse.csr_matrix(counts[row['source_offset']+b['start']:row['source_offset']+b['end']+1].sum(axis=0))
            blocks.append({terms[i]:int(n) for i,n in zip(r.indices,r.data)})
    assert len(blocks)==8196
    ids=[f"{data['source_sha256']}/meeting:{row['meeting']}/block:{b['ordinal']}" for row in geometry['records'] for b in row['blocks']][:32];assert ids==data['bootstrap_identities']
    banks={};states={};coordinate_checks=cases=pair_checks=label_checks=representative_checks=0;maximum_error=0.;ties=[]
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
    for policy,model in data['models'].items():
        df=Counter();seed=[];local_states=[]
        def used(original):
            found={t:n for t,n in original.items() if policy=='marker_only_tf' or t not in excluded}
            return (found,'ready') if found else (original,'filtered_empty_fallback') if original else ({},'empty')
        for identity,original in zip(ids,blocks[:32]):
            selected,state=used(original);df.update(selected.keys());seed.append({'id':identity,'original_counts':original,'state':state})
        assert model=={'version':1,'encoding':preflight['summary'][policy]['descriptor'],'seed_documents':32,
            'term_document_frequencies':dict(df),'seed_sha256':pin(seed),
            'idf':'ln((1+seed_documents)/(1+term_df))+1_unseen_df0',
            'weighted_norm':'sorted_term_scalar_weights_bucket_fsum_L2_fsum','csr_layout':'ascending_unique_indices'}
        for kind in ['hashed','identity_oracle']:
            dimensions=16384 if kind=='hashed' else len(terms);coordinates={t:i for i,t in enumerate(terms)}
            bank=sparse.csr_matrix(tuple(np.load(capture/f'{policy}-{kind}-{k}.npy') for k in ['data','indices','indptr']),shape=(8196,dimensions));banks[policy,kind]=bank
            for i,original in enumerate(blocks):
                selected,state=used(original)
                if kind=='hashed':local_states.append(state)
                values={}
                for term,n in sorted(selected.items()):
                    weight=(1+math.log(n))*(math.log(33/(1+df.get(term,0)))+1)
                    raw=hashlib.sha256(b'mash.topic.lexical.v1\0'+term.encode()).digest()
                    index=sum(raw[k]*256**k for k in range(8))%16384 if kind=='hashed' else coordinates[term]
                    values.setdefault(index,[]).append(weight)
                total={j:math.fsum(v) for j,v in values.items()};norm=math.sqrt(math.fsum(v*v for v in total.values()))
                r=bank[i];assert r.has_sorted_indices and r.indices.tolist()==sorted(total)
                for j,x in zip(r.indices,r.data):assert abs(float(x)-total[j]/norm)<=1e-12;coordinate_checks+=1
            states[policy]=local_states
    def metrics(tree,owners,gold,parts,ordinals,threshold):
        nonlocal pair_checks
        children={int(k):v for k,v in tree['children'].items()};leaves={int(k):v for k,v in tree['leaves'].items()};n=len(leaves);assert sorted(leaves.values())==list(range(n))
        leaf_counts=np.zeros((n,len(meetings[current]['topic_list'])),dtype=np.int64)
        for block,topic in zip(owners,gold):leaf_counts[block,topic]+=1
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
        out={}
        for phase in ['full','post_bootstrap']:
            out[phase]={}
            for scope in ['all','far','crossspan']:
                events=Counter()
                for topic in range(leaf_counts.shape[1]):
                    ids=np.flatnonzero(gold==topic);a,b=np.triu_indices(len(ids),1);a,b=ids[a],ids[b];valid=owners[a]!=owners[b]
                    if phase=='post_bootstrap':valid&=(owners[a]>=threshold)&(owners[b]>=threshold)
                    if scope!='all':valid&=ordinals[b]-ordinals[a]>4
                    if scope=='crossspan':valid&=parts[a]!=parts[b]
                    nodes=lcas[owners[a[valid]],owners[b[valid]]];assert np.all(nodes>=0);unique,pop=np.unique(nodes,return_counts=True)
                    events.update({(int(node),topic):int(p) for node,p in zip(unique,pop)})
                pairs=sum(events.values());weight=math.fsum(p*int(node_counts[node][topic])/int(node_counts[node].sum()) for (node,topic),p in events.items());pair_checks+=pairs
                out[phase][scope]={'pairs':pairs,'purity_sum':weight,'purity':weight/pairs if pairs else None}
        return out,descendants,children
    for current,(meeting,row) in enumerate(zip(meetings,geometry['records'])):
        source_members=[[] for _ in meeting['meeting_transcripts']]
        for topic,r in enumerate(meeting['topic_list']):
            spans=sorted((int(a),int(b)) for a,b in r['relevant_text_span']);assert all(a[1]+1<b[0] for a,b in zip(spans,spans[1:]))
            for component,(a,b) in enumerate(spans):
                for i in range(a,b+1):source_members[i].append((topic,component))
        owner=np.full(len(source_members),-1,dtype=np.int64)
        for b in row['blocks']:owner[b['start']:b['end']+1]=b['ordinal']
        known=np.array([i for i,m in enumerate(source_members) if owner[i]>=0 and len(m)==1]);assert known.tolist()==row['known_source_ordinals']
        gold=np.array([source_members[i][0][0] for i in known]);parts=np.array([source_members[i][0][1] for i in known]);owners=owner[known]
        lo=row['block_directions_offset'];threshold=max(0,32-lo);n=len(row['blocks']);chosen=[r for r in data['records'] if r['meeting']==current];assert len(chosen)==8
        for result in chosen:
            audited,descendants,children=metrics(result['tree'],owners,gold,parts,known,threshold)
            close(audited['full'],{s:{k:result['metrics'][s][k] for k in ['pairs','purity_sum','purity']} for s in audited['full']});close(audited['post_bootstrap'],result['post_bootstrap'])
            order=list(range(n));order=order[::-1] if result['order']=='reverse' else order;assert result['state']['order']==order and result['state']['identities']==list(map(str,order))
            local_states=states[result['policy']][lo:lo+n];assert local_states==result['states'];features=banks[result['policy'],result['kind']][lo:lo+n]
            cut=[result['tree']['root']]
            while len(cut)<min(8,n):
                node=min([v for v in cut if v in children],key=lambda v:(-len(descendants[v]),descendants[v]));cut.remove(node);cut.extend(children[node])
            groups=[descendants[v] for v in sorted(cut,key=lambda v:descendants[v])];assert groups==[v['blocks'] for v in result['display_labels']]
            class_counts=[Counter() for _ in groups]
            for g,c in zip(groups,class_counts):
                for i in g:
                    if local_states[i]=='ready':c.update({t:n for t,n in blocks[lo+i].items() if t not in excluded})
            frequency=sum(class_counts,Counter());lengths=[sum(c.values()) for c in class_counts];average=sum(lengths)/len(lengths)
            for g,c,length,label in zip(groups,class_counts,lengths,result['display_labels']):
                scored=[{'term':t,'score':n/max(length,1)*math.log1p(average/frequency[t])} for t,n in c.items()];wanted=sorted(scored,key=lambda v:(-v['score'],v['term']))[:8];close(wanted,label['labels']);label_checks+=len(wanted)
                assert label['label_eligible_blocks']==[i for i in g if local_states[i]=='ready']
                replay=(features[g]@sparse.csr_matrix(features[g].mean(axis=0)).T).toarray().ravel();winner=min(range(len(g)),key=lambda j:(-replay[j],g[j]));assert g[winner]==label['representative_block']
                means=[math.fsum(v)/len(g) for v in (features[g]@features[g].T).toarray()];assert max(means)-means[winner]<=1e-12
                alternative=min(range(len(g)),key=lambda j:(-means[j],g[j]))
                if alternative!=winner:ties.append({'meeting':current,'policy':result['policy'],'kind':result['kind'],'order':result['order'],'gap':max(means)-means[winner]})
                close(means[winner],label['representative_mean_similarity']);b=row['blocks'][g[winner]]
                assert label['representative_source']=={'start':b['start'],'end':b['end'],'text':'\n'.join(t['content'] for t in meeting['meeting_transcripts'][b['start']:b['end']+1])};representative_checks+=1
            cases+=1
        baselines=[r for r in data['matched_baselines'] if r['meeting']==current];assert len(baselines)==9
        for r in baselines:
            key=(current,r['policy'],r['kind'],r['order']);audited,_,_=metrics(baseline_trees[key],owners,gold,parts,known,threshold)
            close(audited['full'],r['full']);close(audited['post_bootstrap'],r['post_bootstrap'])
        if current%20==0:print(json.dumps({'audited_meetings':current+1}),flush=True)
    assert cases==1296
    for policy,kinds in data['summary'].items():
        for kind,orders in kinds.items():
            for order,phases in orders.items():
                rows=[r for r in data['records'] if (r['policy'],r['kind'],r['order'])==(policy,kind,order)]
                for phase,scopes in phases.items():
                    for scope,wanted in scopes.items():
                        values=[r['metrics' if phase=='full' else phase][scope] for r in rows];pairs=sum(v['pairs'] for v in values);weight=math.fsum(v['purity_sum'] for v in values)
                        bases=[r[phase][scope] for r in data['matched_baselines'] if r['policy']=='offline_tfidf'];assert pairs==sum(v['pairs'] for v in bases);base=math.fsum(v['purity_sum'] for v in bases)
                        close({'pairs':pairs,'purity_sum':weight,'purity':weight/pairs,'offline_reference':base/pairs,'loss_vs_offline':(base-weight)/pairs},wanted)
    return {'verified':True,'capture_sha256':sha(capture/'results.json'),'auditor_sha256':sha(Path(__file__)),'weighted_coordinates':coordinate_checks,'tree_cases':cases,
        'pair_scope_contributions':pair_checks,'label_terms_checked':label_checks,'representatives_checked':representative_checks,
        'representative_reduction_ties':ties,'maximum_numeric_error':maximum_error,
        'limitations':['shared pinned original count ledger, NumPy/SciPy/static sklearn stoplist; source count tokenizer previously audited',
            'independent bootstrap DF/weights/hash coordinates/source memberships/LCA pairs/labels; representative IDs replay shared mean operation and separate pairwise score/max audit',
            'not independent rotation choices or source embedding inference; node aggregate fingerprints not audited here',
            'not semantic/global lifecycle/full-ingestion or selected Rust parity evidence']}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--artifacts',type=Path,required=True);p.add_argument('--capture',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    result=audit(a.artifacts,a.capture)
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps({k:v for k,v in result.items() if k!='representative_reduction_ties'}|{'representative_reduction_tie_count':len(result['representative_reduction_ties'])}),flush=True)


if __name__=='__main__':main()
