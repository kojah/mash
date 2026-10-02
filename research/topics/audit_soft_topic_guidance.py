"""Independent rank/formula/query/span audit with scalar and IEEE feature evidence."""
import argparse
from collections import Counter
import json
import math
from pathlib import Path
import numpy as np
from lexical_navigation_reference import load,digest


def audit(root,path):
    data=json.loads(path.read_text());ref=load(root,data['control_sha256'],data['source_sha256'])
    for name,pin in data['code_pins'].items():assert digest(Path(__file__).with_name(name))==pin
    source_ids=np.flatnonzero(ref.owners>=0);group_ids=np.arange(len(ref.group_meeting),dtype=np.int64)
    summary={s:{a:Counter() for a in ['direct_sources','parent_rrf']} for s in ['meeting','global']};paired={s:Counter() for s in summary}
    maximum=ref.maximum_scalar_error;selected=queries=0
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
        hits=[i+1 for i,r in enumerate(rows) if r['id'] in relevant];dcg=sum(1/math.log2(i+1) for i in hits)
        ideal=sum(1/math.log2(i+1) for i in range(1,min(20,len(relevant))+1));available=len(relevant&set(candidates.tolist()))
        return {'relevant_sources':len(relevant),'hits':len(hits),'recall_at_20':len(hits)/len(relevant),'precision_at_20':len(hits)/20,
            'mrr':1/hits[0] if hits else 0.,'ndcg_at_20':dcg/ideal,'candidate_relevant_sources':available,'candidate_recall':available/len(relevant),
            'returned_sources':len(rows),'zero_results':int(not rows)}
    assert data['support']==ref.control['support'] and len(data['records'])==1095
    for r,prior in zip(data['records'],ref.control['records']):
        assert {k:v for k,v in r.items() if k!='cases'}=={k:v for k,v in prior.items() if k!='cases'}
        meeting=r['meeting'];q=ref.meetings[meeting]['specific_query_list'][r['query']];offset=ref.geometry['records'][meeting]['source_offset']
        relevant={offset+i for lo,hi in q['relevant_text_span'] for i in range(int(lo),int(hi)+1)};assert sorted(relevant)==r['relevant_sources']
        sources,groups,qerror=ref.scores(q['query']);maximum=max(maximum,qerror)
        scopes={'meeting':(source_ids[ref.source_meeting[source_ids]==meeting],group_ids[ref.group_meeting==meeting]),'global':(source_ids,group_ids)}
        for scope,(ids,gids) in scopes.items():
            source_order=order(sources,ids);group_order=order(groups,gids);parent_rank={int(g):i+1 for i,g in enumerate(group_order)}
            direct=[{'id':int(j),'score':float(sources[j])} for j in source_order[:20]];direct_case=r['cases'][scope]['direct_sources']
            assert direct_case==prior['cases'][scope]['direct_sources'];close(direct,direct_case['returned_sources'])
            fused=[]
            for i,j in enumerate(source_order):
                parent=int(ref.owners[j]);pr=parent_rank.get(parent);value=1/(60+i+1)+(1/(60+pr) if pr else 0.)
                fused.append({'id':int(j),'score':value,'source_score':float(sources[j]),'source_rank':i+1,'parent_group':parent,
                    'parent_rank':pr,'parent_score':float(groups[parent]) if pr else 0.})
            chosen=sorted(fused,key=lambda v:(-v['score'],v['id']))[:20];case=r['cases'][scope]['parent_rrf']
            close(chosen,case['returned_sources']);assert case['candidate_sources']==len(source_order) and case['positive_groups']==len(group_order)
            assert len(chosen)==len(direct);selected+=len(chosen)+len(direct)
            for name,rows in [('direct_sources',direct),('parent_rrf',chosen)]:
                m=metric(rows,relevant,ids);close(m,r['cases'][scope][name]['metrics']);summary[scope][name].update(m)
            delta=case['metrics']['recall_at_20']-direct_case['metrics']['recall_at_20'];paired[scope].update(improved=int(delta>0),declined=int(delta<0),tied=int(delta==0))
        queries+=1
        if queries%200==0:print(json.dumps({'audited_queries':queries}),flush=True)
    for scope,arms in summary.items():
        for arm,m in arms.items():summary[scope][arm]={'totals':dict(m),'macro':{k:m[k]/queries for k in ['recall_at_20','precision_at_20','mrr','ndcg_at_20','candidate_recall','returned_sources']}}
    close(summary,data['summary']);close(paired,data['paired_recall'])
    return {'capture_sha256':digest(path),'auditor_sha256':digest(Path(__file__)),'reference_sha256':digest(Path(__file__).with_name('lexical_navigation_reference.py')),
        'queries_checked':queries,'retrieval_cases_checked':queries*4,'selected_sources_checked':selected,'feature_coordinates_checked':ref.coordinates,
        'maximum_scalar_feature_error':ref.maximum_scalar_error,'maximum_numeric_error':maximum,
        'limitations':['independent original count/query/span/ownership/formula/metric control; scalar fsum feature geometry within1e-12',
            'exact rank/selection uses separate formula replay sharing NumPy/SciPy IEEE kernels, not independent scalar tie equivalence',
            'shared pinned counts/IDF/frozen groups and sklearn stoplist; source tokenizer previously audited',
            'not production hybrid/RRF, held-out coding/agent utility, semantic-label/multitag/ancestor quality or ingestion/global lifecycle/Rust parity']}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--artifacts',type=Path,required=True);p.add_argument('--capture',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    result=audit(a.artifacts,a.capture)
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps(result),flush=True)


if __name__=='__main__':main()
