"""Independent scalar label/count/source-support and evaluation-only name audit."""
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
    assert sha(gp)==data['geometry_sha256'] and sha(rp)==data['reference_sha256'];geometry=json.loads(gp.read_text());reference=json.loads(rp.read_text())
    source=root/'topic-qmsum-author-training-20261002/data/ALL/jsonl/train.jsonl';assert sha(source)==data['source_sha256'];meetings=[json.loads(line) for line in source.read_bytes().splitlines()]
    for name,pin in data['code_pins'].items():assert sha(Path(__file__).with_name(name))==pin
    lr=root/'topic-qmsum-label-cleanup-first-20261002';lp=lr/'results.json';assert sha(lp)=='064ff3731955b515a07e12e3897102e2cd9d137ac91c43ec222e393a2e492d08'
    ledger=json.loads(lp.read_text());schema=ledger['matrices']['exact_markers']
    for name,pin in schema['files'].items():assert sha(lr/name)==pin
    counts=sparse.csr_matrix(tuple(np.load(lr/f'exact_markers-{k}.npy') for k in ['data','indices','indptr']),shape=tuple(schema['shape']))
    vr=root/'topic-qmsum-lexical-context-final-first-20261002';baseline=json.loads((vr/'results.json').read_text());assert sha(vr/'results.json')==ledger['baseline_sha256'];assert sha(vr/'vocabulary.json')==baseline['vocabulary_sha256'];terms=json.loads((vr/'vocabulary.json').read_text());del ledger,baseline
    excluded=frozenset(ENGLISH_STOP_WORDS)|{'yeah','okay','ok','uh','um','erm','hmm','mm','oh'};blocks=[];df=Counter();frequency=Counter()
    for g in geometry['records']:
        for b in g['blocks']:
            row=sparse.csr_matrix(counts[g['source_offset']+b['start']:g['source_offset']+b['end']+1].sum(axis=0))
            words={terms[j]:int(v) for j,v in zip(row.indices,row.data)};blocks.append(words);df.update(words.keys());frequency.update({t:n for t,n in words.items() if t not in excluded})
    total=sum(frequency.values());maximum=0.;labels_checked=support_examples=source_checks=0;aggregates={name:Counter() for name in ['corpus_idf_ctfidf','relevance_0_6']};group_stats={name:Counter() for name in aggregates}
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
        assert r['meeting']==ordinal;old=next(v for v in reference['records'] if v['meeting']==ordinal and v['method']=='lexical_average');tree=old['tree'];children={int(k):v for k,v in tree['children'].items()};leaves={int(k):v for k,v in tree['leaves'].items()}
        descendants={i:[v] for i,v in leaves.items()};pending=list(children)
        while pending:
            progress=False
            for node in pending.copy():
                a,b=children[node]
                if a in descendants and b in descendants:descendants[node]=sorted(descendants[a]+descendants[b]);pending.remove(node);progress=True
            assert progress
        cut=[tree['root']]
        while len(cut)<min(8,len(leaves)):
            node=min((v for v in cut if v in children),key=lambda v:(-len(descendants[v]),descendants[v]));cut.remove(node);cut.extend(children[node])
        groups=sorted(descendants[n] for n in cut);assert groups==r['groups'];lo=g['block_directions_offset'];local=[{t:n for t,n in b.items() if t not in excluded} for b in blocks[lo:lo+len(leaves)]]
        states=['ready' if b else 'filtered_empty_fallback' for b in local];assert states==r['states']
        classes=[sum((Counter(local[j]) for j in group),Counter()) for group in groups];class_frequency=sum(classes,Counter());lengths=[sum(c.values()) for c in classes];average=sum(lengths)/len(lengths);wanted={name:[] for name in aggregates}
        for group,c,length in zip(groups,classes,lengths):
            member_df=Counter(t for j in group for t in local[j]);a=[];b=[]
            for t,n in c.items():
                if not any(ch.isalpha() for ch in t) or member_df[t]<2 or df[t]>.4*len(blocks):continue
                base=n/max(length,1)*math.log1p(average/class_frequency[t])*(math.log((1+len(blocks))/(1+df[t]))+1)
                a.append({'term':t,'score':base,'member_blocks':member_df[t],'background_blocks':df[t]})
                p=n/length;q=frequency[t]/total;b.append({'term':t,'score':.6*math.log(p)+.4*math.log(p/q),
                    'member_blocks':member_df[t],'background_blocks':df[t],'within_probability':p,'background_probability':q,'lift':p/q})
            for name,candidates in [('corpus_idf_ctfidf',a),('relevance_0_6',b)]:wanted[name].append(sorted(candidates,key=lambda t:(-t['score'],t['term']))[:8])
        for name,rows in wanted.items():
            close(rows,r['labels'][name]);labels_checked+=sum(map(len,rows));examples=[]
            for group,row in zip(groups,rows):
                examples.append([{'term':t['term'],'blocks':[j for j in group if t['term'] in local[j]][:2]} for t in row]);support_examples+=len(row)
                group_stats[name].update(groups=1,empty_groups=int(not row),weak_groups=int(len(row)<2),label_terms=len(row))
            assert examples==r['label_support_examples'][name]
        membership=[[] for _ in meeting['meeting_transcripts']]
        names=[]
        for topic,entry in enumerate(meeting['topic_list']):
            names.append(set(t for t in token.findall(entry['topic'].lower()) if t not in excluded and any(c.isalpha() for c in t)))
            for start,end in entry['relevant_text_span']:
                for i in range(int(start),int(end)+1):
                    if topic not in membership[i]:membership[i].append(topic)
        assignment={block:i for i,group in enumerate(groups) for block in group};owner=np.full(len(membership),-1,dtype=np.int64)
        for b in g['blocks']:owner[b['start']:b['end']+1]=b['ordinal']
        for name,rows in wanted.items():
            metric=Counter(named_sources=0,empty_name_sources=0,unknown_or_mixed_sources=0,hits=0)
            for i in np.flatnonzero(owner>=0):
                source_checks+=1
                if len(membership[i])!=1:metric['unknown_or_mixed_sources']+=1;continue
                topic=membership[i][0]
                if not names[topic]:metric['empty_name_sources']+=1;continue
                metric['named_sources']+=1;metric['hits']+=bool({t['term'] for t in rows[assignment[int(owner[i])]]}&names[topic])
            close(dict(metric)|{'rate':metric['hits']/metric['named_sources'] if metric['named_sources'] else None},r['name_proxy'][name]);aggregates[name].update(metric)
    for name,m in aggregates.items():close({'name_proxy':dict(m)|{'rate':m['hits']/m['named_sources'] if m['named_sources'] else None},**dict(group_stats[name])},data['summary'][name])
    assert len(data['records'])==162
    return {'capture_sha256':sha(path),'auditor_sha256':sha(Path(__file__)),'label_terms_checked':labels_checked,'support_examples_checked':support_examples,
        'source_name_proxy_checks':source_checks,'maximum_numeric_error':maximum,
        'limitations':['shared pinned original count ledger/NumPy/SciPy/sklearn static stoplist; original tokenizer previously audited',
            'independent scalar formulas/ranking/support, fixed group cut and original annotations/name-token evaluation; no inferred semantic label accuracy',
            'not membership quality, semantic ancestry, coding/guided-search, full-ingestion/global lifecycle or Rust parity proof']}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--artifacts',type=Path,required=True);p.add_argument('--capture',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    result=audit(a.artifacts,a.capture)
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps(result),flush=True)


if __name__=='__main__':main()
