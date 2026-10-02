"""Scalar source-presence/count/score/rank audit without production label imports."""
import argparse
from collections import Counter
import hashlib
import json
import math
from pathlib import Path
import re
import numpy as np
from scipy import sparse
from sklearn.feature_extraction.text import ENGLISH_STOP_WORDS


def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()


def audit(root,path):
    data=json.loads(path.read_text());gp=root/'topic-qmsum-block-geometry-first-20261002/results.json';rp=root/'topic-qmsum-block-hierarchy-final-first-20261002.json'
    cp=root/'topic-qmsum-label-relevance-first-20261002.json'
    assert sha(gp)==data['geometry_sha256'] and sha(rp)==data['reference_sha256'] and sha(cp)==data['control_sha256']
    geometry=json.loads(gp.read_text());reference=json.loads(rp.read_text())
    source=root/'topic-qmsum-author-training-20261002/data/ALL/jsonl/train.jsonl';assert sha(source)==data['source_sha256'];meetings=[json.loads(line) for line in source.read_bytes().splitlines()]
    for name,pin in data['code_pins'].items():assert sha(Path(__file__).with_name(name))==pin
    lr=root/'topic-qmsum-label-cleanup-first-20261002';lp=lr/'results.json';assert sha(lp)=='064ff3731955b515a07e12e3897102e2cd9d137ac91c43ec222e393a2e492d08'
    ledger=json.loads(lp.read_text());schema=ledger['matrices']['exact_markers']
    for name,pin in schema['files'].items():assert sha(lr/name)==pin
    counts=sparse.csr_matrix(tuple(np.load(lr/f'exact_markers-{k}.npy') for k in ['data','indices','indptr']),shape=tuple(schema['shape']))
    vr=root/'topic-qmsum-lexical-context-final-first-20261002';baseline=json.loads((vr/'results.json').read_text())
    assert sha(vr/'results.json')==ledger['baseline_sha256'] and sha(vr/'vocabulary.json')==baseline['vocabulary_sha256']
    terms=json.loads((vr/'vocabulary.json').read_text());del ledger,baseline
    excluded=frozenset(ENGLISH_STOP_WORDS)|{'yeah','okay','ok','uh','um','erm','hmm','mm','oh'}
    original=[]
    for i in range(counts.shape[0]):
        row=counts[i];original.append({terms[j]:int(v) for j,v in zip(row.indices,row.data)})
    blocks=[];df=Counter()
    for g in geometry['records']:
        for b in g['blocks']:
            words=sum((Counter(original[g['source_offset']+i]) for i in range(b['start'],b['end']+1)),Counter())
            blocks.append(dict(words));df.update(words.keys())
    maximum=0.;checked=examples_checked=source_checks=0;names=['physical_block','original_message']
    aggregates={name:Counter() for name in names};group_stats={name:Counter() for name in names};support=Counter()
    def close(a,b):
        nonlocal maximum
        if isinstance(a,dict):
            assert a.keys()==b.keys()
            for key in a:close(a[key],b[key])
        elif isinstance(a,list):
            assert len(a)==len(b)
            for x,y in zip(a,b):close(x,y)
        elif isinstance(a,float):maximum=max(maximum,abs(a-b));assert math.isclose(a,b,rel_tol=1e-12,abs_tol=1e-12),(a,b)
        else:assert a==b,(a,b)
    token=re.compile(r'(?u)\b\w\w+\b')
    for ordinal,(meeting,g,r) in enumerate(zip(meetings,geometry['records'],data['records'])):
        offset=g['source_offset'];assert r['meeting']==ordinal and r['source_offset']==offset
        old=next(v for v in reference['records'] if v['meeting']==ordinal and v['method']=='lexical_average');tree=old['tree']
        children={int(k):v for k,v in tree['children'].items()};descendants={int(k):[v] for k,v in tree['leaves'].items()};pending=list(children)
        while pending:
            progress=False
            for node in pending.copy():
                a,b=children[node]
                if a in descendants and b in descendants:descendants[node]=sorted(descendants[a]+descendants[b]);pending.remove(node);progress=True
            assert progress
        cut=[tree['root']]
        while len(cut)<min(8,len(tree['leaves'])):
            node=min((v for v in cut if v in children),key=lambda v:(-len(descendants[v]),descendants[v]));cut.remove(node);cut.extend(children[node])
        groups=sorted(descendants[n] for n in cut);assert groups==r['groups']
        lo=g['block_directions_offset'];local=[{t:n for t,n in b.items() if t not in excluded} for b in blocks[lo:lo+len(g['blocks'])]]
        states=['ready' if b else 'filtered_empty_fallback' for b in local];assert states==r['states']
        owner=[-1]*len(meeting['meeting_transcripts'])
        for b in g['blocks']:
            for j in range(b['start'],b['end']+1):assert owner[j]==-1;owner[j]=b['ordinal']
        sources=[[offset+i for i,b in enumerate(owner) if b in group] for group in groups]
        assert list(map(len,sources))==r['group_ready_sources']
        classes=[sum((Counter(local[j]) for j in group),Counter()) for group in groups]
        class_frequency=sum(classes,Counter());lengths=[sum(c.values()) for c in classes];average=sum(lengths)/len(lengths);wanted={name:[] for name in names}
        for group,c,length,members in zip(groups,classes,lengths,sources):
            block_df=Counter(t for j in group for t in local[j])
            source_df=Counter(t for i in members if states[owner[i-offset]]=='ready' for t in original[i] if t not in excluded)
            for name in names:
                member=block_df if name=='physical_block' else source_df;candidates=[]
                for t,n in c.items():
                    if not any(ch.isalpha() for ch in t) or member[t]<2 or df[t]>.4*len(blocks):continue
                    value=n/max(length,1)*math.log1p(average/class_frequency[t])*(math.log((1+len(blocks))/(1+df[t]))+1)
                    candidates.append({'term':t,'score':value,'member_blocks':block_df[t],'member_sources':source_df[t],'background_blocks':df[t]})
                wanted[name].append(sorted(candidates,key=lambda t:(-t['score'],t['term']))[:8])
        for name,rows in wanted.items():
            close(rows,r['labels'][name]);checked+=sum(map(len,rows));examples=[]
            for group,row,old,members in zip(groups,rows,wanted['physical_block'],sources):
                examples.append([{'term':t['term'],'original_sources':[i for i in members if states[owner[i-offset]]=='ready' and t['term'] in original[i]][:2]} for t in row]);examples_checked+=len(row)
                group_stats[name].update(groups=1,empty_groups=int(not row),weak_groups=int(len(row)<2),label_terms=len(row),
                    recovered_empty_groups=int(not old and bool(row)),recovered_two_term_groups=int(len(old)<2<=len(row)),
                    single_block_groups=int(len(group)==1),single_original_source_groups=int(len(members)==1))
            assert examples==r['support_examples'][name]
        membership=[[] for _ in owner];native=[]
        for topic,entry in enumerate(meeting['topic_list']):
            native.append(set(t for t in token.findall(entry['topic'].lower()) if t not in excluded and any(c.isalpha() for c in t)))
            for start,end in entry['relevant_text_span']:
                for i in range(int(start),int(end)+1):
                    if topic not in membership[i]:membership[i].append(topic)
        assignment={b:i for i,group in enumerate(groups) for b in group}
        for name,rows in wanted.items():
            metric=Counter(named_sources=0,empty_name_sources=0,unknown_or_mixed_sources=0,hits=0)
            for i,b in enumerate(owner):
                if b<0:continue
                source_checks+=1
                if len(membership[i])!=1:metric['unknown_or_mixed_sources']+=1;continue
                topic=membership[i][0]
                if not native[topic]:metric['empty_name_sources']+=1;continue
                metric['named_sources']+=1;metric['hits']+=bool({t['term'] for t in rows[assignment[b]]}&native[topic])
            close(dict(metric)|{'rate':metric['hits']/metric['named_sources'] if metric['named_sources'] else None},r['name_proxy'][name]);aggregates[name].update(metric)
        support.update(sources=len(owner),ready=sum(b>=0 for b in owner),known=sum(b>=0 and len(membership[i])==1 for i,b in enumerate(owner)),blocks=len(g['blocks']),groups=len(groups))
    for name,m in aggregates.items():close({'name_proxy':dict(m)|{'rate':m['hits']/m['named_sources']},'diagnostics':dict(group_stats[name])},data['summary'][name])
    assert len(data['records'])==len(meetings)==162 and dict(support)==data['support']
    return {'capture_sha256':sha(path),'auditor_sha256':sha(Path(__file__)),'label_terms_checked':checked,'support_examples_checked':examples_checked,
        'source_name_proxy_checks':source_checks,'maximum_numeric_error':maximum,
        'limitations':['shared pinned original count ledger/NumPy/SciPy/sklearn static stoplist; original tokenizer previously audited',
            'independent source presence, scalar formulas/ranking, fixed cut and native annotations/name-token evaluation',
            'not semantic labels, membership/ancestry, coding/guided-search, ingestion/global lifecycle or Rust parity proof']}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--artifacts',type=Path,required=True);p.add_argument('--capture',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    result=audit(a.artifacts,a.capture)
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps(result),flush=True)


if __name__=='__main__':main()
