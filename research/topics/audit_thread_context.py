"""Independent original-count thread pooling and three-arm rank/span audit."""
import argparse
from collections import Counter
import json
import math
from pathlib import Path
import numpy as np
from scipy import sparse
from lexical_navigation_reference import load,digest,features,EXCLUDED


def audit(root,path):
    data=json.loads(path.read_text());ref=load(root,data['control_sha256'],data['source_sha256'])
    rp=root/'topic-qmsum-soft-guidance-first-20261002.json';assert digest(rp)==data['reference_capture_sha256'];reference=json.loads(rp.read_text())
    for name,pin in data['code_pins'].items():assert digest(Path(__file__).with_name(name))==pin
    lp=root/'topic-qmsum-label-cleanup-first-20261002/results.json';assert digest(lp)=='064ff3731955b515a07e12e3897102e2cd9d137ac91c43ec222e393a2e492d08'
    ledger=json.loads(lp.read_text());schema=ledger['matrices']['exact_markers']
    for name,pin in schema['files'].items():assert digest(lp.parent/name)==pin
    counts=sparse.csr_matrix(tuple(np.load(lp.parent/f'exact_markers-{k}.npy') for k in ['data','indices','indptr']),shape=tuple(schema['shape']));del ledger
    clean=counts.multiply([t not in EXCLUDED for t in ref.terms]).tocsr();clean.eliminate_zeros();ready=np.flatnonzero(ref.owners>=0)
    incidence=sparse.csr_matrix((np.ones(len(ready)),(ref.source_meeting[ready],ready)),shape=(len(ref.meetings),len(ref.owners)))
    thread_vectors,thread_coordinates,thread_error=features((incidence@clean).tocsr(),ref.idf)
    maximum=max(ref.maximum_scalar_error,thread_error);selected=queries=0;gids=np.arange(len(ref.group_meeting),dtype=np.int64);tids=np.arange(len(ref.meetings),dtype=np.int64)
    summary={s:{a:Counter() for a in ['direct_sources','topic_parent','thread_parent']} for s in ['meeting','global']};paired={s:{a:Counter() for a in ['direct_sources','thread_parent']} for s in summary}
    def close(a,b):
        nonlocal maximum
        if isinstance(a,dict):
            assert a.keys()==b.keys()
            for k in a:close(a[k],b[k])
        elif isinstance(a,list):
            assert len(a)==len(b)
            for x,y in zip(a,b):close(x,y)
        elif isinstance(a,float):maximum=max(maximum,abs(a-b));assert math.isclose(a,b,rel_tol=1e-12,abs_tol=1e-12),(a,b)
        else:assert a==b,(a,b)
    def order(scores,ids):
        positive=ids[scores[ids]>0];return positive[np.lexsort((positive,-scores[positive]))]
    def metric(rows,relevant,candidates):
        hits=[i+1 for i,row in enumerate(rows) if row['id'] in relevant];dcg=sum(1/math.log2(i+1) for i in hits)
        ideal=sum(1/math.log2(i+1) for i in range(1,min(20,len(relevant))+1));available=len(relevant&set(candidates.tolist()))
        return {'relevant_sources':len(relevant),'hits':len(hits),'recall_at_20':len(hits)/len(relevant),'precision_at_20':len(hits)/20,'mrr':1/hits[0] if hits else 0.,
            'ndcg_at_20':dcg/ideal,'candidate_relevant_sources':available,'candidate_recall':available/len(relevant),'returned_sources':len(rows),'zero_results':int(not rows)}
    for r,previous in zip(data['records'],reference['records']):
        assert {k:v for k,v in r.items() if k!='cases'}=={k:v for k,v in previous.items() if k!='cases'}
        meeting=r['meeting'];q=ref.meetings[meeting]['specific_query_list'][r['query']];offset=ref.geometry['records'][meeting]['source_offset']
        relevant={offset+i for lo,hi in q['relevant_text_span'] for i in range(int(lo),int(hi)+1)};assert sorted(relevant)==r['relevant_sources']
        sources,topics,qerror=ref.scores(q['query']);maximum=max(maximum,qerror)
        # Sum actual ready-source counts into thread rows; no means of already
        # normalized vectors. Query construction is independently checked in ref.
        from lexical_navigation_reference import TOKEN,MARKER
        words=Counter(TOKEN.findall(q['query'].lower()))
        for marker in MARKER.finditer(q['query']):words[marker.group(1)]-=1
        positions={t:i for i,t in enumerate(ref.terms)};known=sorted((positions[t],n) for t,n in words.items() if n and t not in EXCLUDED and t in positions)
        row=sparse.csr_matrix(([n for j,n in known],([0]*len(known),[j for j,n in known])),shape=(1,len(ref.terms)))
        direction,_,qerror=features(row,ref.idf);maximum=max(maximum,qerror);threads=(thread_vectors@direction.T).toarray().ravel()
        scopes={'meeting':(ready[ref.source_meeting[ready]==meeting],gids[ref.group_meeting==meeting],np.array([meeting],dtype=np.int64)),
            'global':(ready,gids,tids)}
        for scope,(ids,topic_ids,thread_ids) in scopes.items():
            source_order=order(sources,ids);direct=[{'id':int(j),'score':float(sources[j])} for j in source_order[:20]]
            assert r['cases'][scope]['direct_sources']==previous['cases'][scope]['direct_sources'];close(direct,r['cases'][scope]['direct_sources']['returned_sources'])
            wanted={'direct_sources':direct}
            for arm,owner,scores,parent_ids in [('topic_parent',ref.owners,topics,topic_ids),('thread_parent',ref.source_meeting,threads,thread_ids)]:
                parent_order=order(scores,parent_ids);ranks={int(j):i+1 for i,j in enumerate(parent_order)};all_rows=[]
                for i,j in enumerate(source_order):
                    parent=int(owner[j]);pr=ranks.get(parent);value=1/(60+i+1)+(1/(60+pr) if pr else 0.)
                    all_rows.append({'id':int(j),'score':value,'source_score':float(sources[j]),'source_rank':i+1,'parent_group':parent,'parent_rank':pr,'parent_score':float(scores[parent]) if pr else 0.})
                chosen=sorted(all_rows,key=lambda row:(-row['score'],row['id']))[:20];case=r['cases'][scope][arm]
                close(chosen,case['returned_sources']);assert case['candidate_sources']==len(source_order) and case['positive_groups']==len(parent_order)
                wanted[arm]=chosen
            assert r['cases'][scope]['topic_parent']==previous['cases'][scope]['parent_rrf']
            if scope=='meeting':assert [row['id'] for row in wanted['thread_parent']]==[row['id'] for row in direct]
            for arm,rows in wanted.items():
                m=metric(rows,relevant,ids);close(m,r['cases'][scope][arm]['metrics']);summary[scope][arm].update(m);selected+=len(rows)
            for baseline,pairs in paired[scope].items():
                delta=r['cases'][scope]['topic_parent']['metrics']['recall_at_20']-r['cases'][scope][baseline]['metrics']['recall_at_20'];pairs.update(improved=int(delta>0),declined=int(delta<0),tied=int(delta==0))
        queries+=1
        if queries%200==0:print(json.dumps({'audited_queries':queries}),flush=True)
    assert queries==len(data['records'])==1095
    for scope,arms in summary.items():
        for arm,m in arms.items():summary[scope][arm]={'totals':dict(m),'macro':{k:m[k]/queries for k in ['recall_at_20','precision_at_20','mrr','ndcg_at_20','candidate_recall','returned_sources']}}
    close(summary,data['summary']);close(paired,data['paired_topic_recall'])
    close(data['support'],reference['support']|{'thread_parents':162,'topic_parents':1296})
    return {'capture_sha256':digest(path),'auditor_sha256':digest(Path(__file__)),'reference_sha256':digest(Path(__file__).with_name('lexical_navigation_reference.py')),
        'queries_checked':queries,'retrieval_cases_checked':queries*6,'selected_sources_checked':selected,'feature_coordinates_checked':ref.coordinates+thread_coordinates,
        'maximum_scalar_feature_error':max(ref.maximum_scalar_error,thread_error),'maximum_numeric_error':maximum,
        'limitations':['independent original count/thread pooling/query/span/ownership/formula/metric checks,scalar fsum geometry within1e-12',
            'exact ranks/selection use separate formula replay sharing NumPy/SciPy IEEE kernels,not independent scalar tie-winner equivalence',
            'shared pinned counts/IDF/frozen topic inventory/sklearn stoplist; source tokenizer previously audited',
            'training meetings/global unjudged-source caveats; not semantic labels/ancestors or held-out coding/agent utility,online lifecycle/ingestion/Rust parity']}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--artifacts',type=Path,required=True);p.add_argument('--capture',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    result=audit(a.artifacts,a.capture)
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps(result),flush=True)


if __name__=='__main__':main()
