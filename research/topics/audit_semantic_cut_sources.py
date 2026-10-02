"""Independent original-annotation pair, scalar label and representative audit."""
import argparse
from collections import Counter
import hashlib
import json
import math
from pathlib import Path
import numpy as np
from scipy import sparse
from sklearn.feature_extraction.text import ENGLISH_STOP_WORDS
from audit_semantic_cut_geometry import audit as geometry_audit


def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()


def ratios(gold,predicted,true):
    return {'gold_pairs':gold,'predicted_pairs':predicted,'true_positive_pairs':true,
        'precision':true/predicted if predicted else None,'recall':true/gold if gold else None,
        'f1':2*true/(predicted+gold) if predicted+gold else None}


def audit(root,path):
    geometry_result=geometry_audit(root,path)
    data=json.loads(path.read_text());gp=root/'topic-qmsum-block-geometry-first-20261002/results.json';geometry=json.loads(gp.read_text())
    source=root/'topic-qmsum-author-training-20261002/data/ALL/jsonl/train.jsonl'
    assert sha(source)==data['source_sha256'];meetings=[json.loads(x) for x in source.read_bytes().splitlines()]
    ledger_root=root/'topic-qmsum-label-cleanup-first-20261002';lp=ledger_root/'results.json'
    assert sha(lp)=='064ff3731955b515a07e12e3897102e2cd9d137ac91c43ec222e393a2e492d08'
    ledger=json.loads(lp.read_text());schema=ledger['matrices']['exact_markers']
    for name,pin in schema['files'].items():assert sha(ledger_root/name)==pin
    counts=sparse.csr_matrix(tuple(np.load(ledger_root/f'exact_markers-{k}.npy') for k in ['data','indices','indptr']),shape=tuple(schema['shape']))
    vr=root/'topic-qmsum-lexical-context-final-first-20261002';vocab_ledger=json.loads((vr/'results.json').read_text())
    assert sha(vr/'results.json')==ledger['baseline_sha256'];assert sha(vr/'vocabulary.json')==vocab_ledger['vocabulary_sha256']
    terms=json.loads((vr/'vocabulary.json').read_text());del ledger,vocab_ledger
    vectors=np.load(gp.parent/'block-directions.npy',mmap_mode='r',allow_pickle=False)
    excluded=frozenset(ENGLISH_STOP_WORDS)|{'yeah','okay','ok','uh','um','erm','hmm','mm','oh'}
    blocks=[];df=Counter()
    for row in geometry['records']:
        for b in row['blocks']:
            r=sparse.csr_matrix(counts[row['source_offset']+b['start']:row['source_offset']+b['end']+1].sum(axis=0))
            words={terms[j]:int(n) for j,n in zip(r.indices,r.data)};blocks.append(words);df.update(words.keys())
    assert len(blocks)==8196
    controls={}
    for frame,p in [('fixed_tf',root/'topic-qmsum-stable-frame-quality-first-20261002.json'),
                    ('calibrated_32',root/'topic-qmsum-calibration-final-first-20261002/results.json')]:
        assert sha(p)==data['control_sha256'][frame]
        controls[frame]={(r['meeting'],r['order']):r for r in json.loads(p.read_text())['records'] if r['policy']=='static_noise_filtered_tf' and r['kind']=='hashed'}
    maximum=0.;label_terms=representatives=pair_cases=eligible_checks=0;ties=[];audited={}
    def close(a,b):
        nonlocal maximum
        if isinstance(a,dict):
            assert a.keys()==b.keys(),(a.keys(),b.keys())
            for key in a:close(a[key],b[key])
        elif isinstance(a,list):
            assert len(a)==len(b)
            for x,y in zip(a,b):close(x,y)
        elif isinstance(a,float):
            maximum=max(maximum,abs(a-b));assert math.isclose(a,b,rel_tol=1e-12,abs_tol=1e-12),(a,b)
        else:assert a==b,(a,b)
    for ordinal,(meeting,g) in enumerate(zip(meetings,geometry['records'])):
        membership=[[] for _ in meeting['meeting_transcripts']]
        for topic,entry in enumerate(meeting['topic_list']):
            spans=sorted((int(a),int(b)) for a,b in entry['relevant_text_span'])
            assert all(a[1]+1<b[0] for a,b in zip(spans,spans[1:]))
            for component,(a,b) in enumerate(spans):
                for i in range(a,b+1):membership[i].append((topic,component))
        owner=np.full(len(membership),-1,dtype=np.int64)
        for b in g['blocks']:owner[b['start']:b['end']+1]=b['ordinal']
        ready=np.flatnonzero(owner>=0);known=np.array([i for i in ready if len(membership[i])==1]);assert known.tolist()==g['known_source_ordinals']
        lo=g['block_directions_offset'];n=len(g['blocks']);v=vectors[lo:lo+n];gram=v@v.T
        topic=np.array([membership[i][0][0] for i in known]);component=np.array([membership[i][0][1] for i in known]);owned=owner[known]
        a,b=np.triu_indices(len(known),1);different_blocks=owned[a]!=owned[b];same_topic=topic[a]==topic[b];far=known[b]-known[a]>4
        masks={'all':different_blocks,'far':different_blocks&far,'crossspan':different_blocks&far&(~same_topic|(component[a]!=component[b]))}
        local=[{t:c for t,c in block.items() if t not in excluded} for block in blocks[lo:lo+n]]
        states=['ready' if w else 'filtered_empty_fallback' for w in local]
        chosen=[r for r in data['records'] if r['meeting']==ordinal];assert len(chosen)==16
        for r in chosen:
            old=controls[r['frame']][ordinal,r['order']];assert states==old['states'];assert r['unchanged_uncut_metrics']==old['metrics']
            groups=[p['blocks'] for p in r['proposals']];assert groups==sorted(groups);assert len(sum(groups,[]))==len(set(sum(groups,[])))
            if r['cut']=='fixed_eight':
                children={int(k):x for k,x in old['tree']['children'].items()};leaves={int(k):x for k,x in old['tree']['leaves'].items()}
                def descendants(node):return [leaves[node]] if node in leaves else sorted(sum((descendants(c) for c in children[node]),[]))
                cut=[old['tree']['root']]
                while len(cut)<min(8,n):
                    split=min((node for node in cut if node in children),key=lambda node:(-len(descendants(node)),descendants(node)))
                    cut.remove(split);cut.extend(children[split])
                assert groups==sorted(descendants(node) for node in cut)
            else:
                sibling=next(x for x in chosen if x['frame']==r['frame'] and x['order']==r['order'] and x['cut']=='semantic' and x['profile']=='static_noise_ctfidf')
                assert groups==sibling['semantic_cut']['groups']
            classes=[sum((Counter(local[j]) for j in group),Counter()) for group in groups];frequency=sum(classes,Counter());lengths=[sum(c.values()) for c in classes];average=sum(lengths)/len(lengths) if lengths else 0.
            usable=[]
            for group,words,length,p in zip(groups,classes,lengths,r['proposals']):
                member_df=Counter(t for j in group for t in local[j]);expected=[]
                for t,tf in words.items():
                    if r['profile']=='source_supported_corpus_idf' and (not any(c.isalpha() for c in t) or member_df[t]<2 or df[t]>.4*len(blocks)):continue
                    score=tf/max(length,1)*math.log1p(average/frequency[t])
                    if r['profile']=='source_supported_corpus_idf':score*=math.log((1+len(blocks))/(1+df[t]))+1
                    expected.append({'term':t,'score':score,'member_blocks':member_df[t],'background_blocks':df[t]})
                wanted=sorted(expected,key=lambda x:(-x['score'],x['term']))[:8];close(wanted,p['labels']);label_terms+=len(wanted)
                assert p['eligible_blocks']==[j for j in group if states[j]=='ready']
                supported=len(wanted)>=2;assert p['support']==('at_least_two_source_supported_label_terms' if supported else 'insufficient_label_terms')
                if supported:usable.append(group)
                replay=v[group]@v[group].mean(axis=0);winner=group[int(np.argmax(replay))];assert winner==p['representative_block'];close(float(replay.max()),p['representative_score'])
                scores=[math.fsum(float(gram[j,k]) for k in group)/len(group) for j in group];index=group.index(winner)
                assert max(scores)-scores[index]<=1e-12;alternative=group[min(range(len(group)),key=lambda j:(-scores[j],group[j]))]
                if alternative!=winner:ties.append({'meeting':ordinal,'frame':r['frame'],'order':r['order'],'cut':r['cut'],'profile':r['profile'],'gap':max(scores)-scores[index]})
                block=g['blocks'][winner];assert p['representative_source']=={'start':block['start'],'end':block['end'],'text':'\n'.join(t['content'] for t in meeting['meeting_transcripts'][block['start']:block['end']+1])};representatives+=1
            for phase,selected in [('proposed',groups),('usable',usable)]:
                assignment={block:j for j,group in enumerate(selected) for block in group};assigned=np.array([assignment.get(int(block),-1) for block in owned]);pred=(assigned[a]>=0)&(assigned[a]==assigned[b]);metrics={}
                for scope,mask in masks.items():
                    metrics[scope]=ratios(int((mask&same_topic).sum()),int((mask&pred).sum()),int((mask&same_topic&pred).sum()));eligible_checks+=int(mask.sum())
                metrics['coverage']={'ready_sources':len(ready),'singleton_known_sources':len(known),'assigned_sources':sum(int(owner[i]) in assignment for i in ready),
                    'assigned_known_sources':sum(int(owner[i]) in assignment for i in known),'physical_blocks':n,'assigned_blocks':len(assignment),'unassigned_blocks':n-len(assignment),
                    'groups':len(selected),'group_sizes':sorted(map(len,selected))}
                close(metrics,r[f'{phase}_metrics']);audited[ordinal,r['frame'],r['order'],r['cut'],r['profile'],phase]=metrics;pair_cases+=1
        if ordinal%20==0:print(json.dumps({'audited_meetings':ordinal+1}),flush=True)
    for summary in data['summary']:
        for phase in ['proposed','usable']:
            rows=[audited[i,summary['frame'],summary['order'],summary['cut'],summary['profile'],phase] for i in range(162)];wanted={}
            for scope in masks:
                wanted[scope]=ratios(*(sum(row[scope][k] for row in rows) for k in ['gold_pairs','predicted_pairs','true_positive_pairs']))
            coverage=Counter()
            for row in rows:coverage.update({k:v for k,v in row['coverage'].items() if k!='group_sizes'})
            wanted.update(coverage=dict(coverage),group_sizes=sorted(size for row in rows for size in row['coverage']['group_sizes']));close(wanted,summary[phase])
    assert pair_cases==5184
    return {'capture_sha256':sha(path),'auditor_sha256':sha(Path(__file__)),'geometry_audit':geometry_result,
        'pair_cases':pair_cases,'eligible_pair_scope_checks':eligible_checks,'label_terms':label_terms,'representatives':representatives,
        'representative_reduction_ties':ties,'maximum_numeric_error':maximum,
        'limitations':['shared pinned source-count ledger, NumPy/SciPy/static sklearn stoplist; no new inference',
            'independent original annotations, scalar labels, pair masks and summaries; representative IDs replay mean operation with separate direct-pair score check',
            'not independent tree fitting or tokenizer; no semantic coding multitag, ancestry, guided-search or full-ingestion/Rust parity evidence']}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--artifacts',type=Path,required=True);p.add_argument('--capture',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    result=audit(a.artifacts,a.capture)
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps({k:v for k,v in result.items() if k!='representative_reduction_ties'}|{'representative_reduction_tie_count':len(result['representative_reduction_ties'])}),flush=True)


if __name__=='__main__':main()
