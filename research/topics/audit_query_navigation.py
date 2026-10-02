"""Independent scalar lexical features/query ranks and original-span metrics."""
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
    result=json.loads(path.read_text());cp=root/'topic-qmsum-source-support-first-20261002.json'
    assert sha(cp)==result['control_sha256'];control=json.loads(cp.read_text())
    source=root/'topic-qmsum-author-training-20261002/data/ALL/jsonl/train.jsonl'
    assert sha(source)==result['source_sha256'];meetings=[json.loads(line) for line in source.read_bytes().splitlines()]
    gp=root/'topic-qmsum-block-geometry-first-20261002/results.json';assert sha(gp)==control['geometry_sha256'];geometry=json.loads(gp.read_text())
    for name,pin in result['code_pins'].items():assert sha(Path(__file__).with_name(name))==pin
    lr=root/'topic-qmsum-label-cleanup-first-20261002';lp=lr/'results.json';assert sha(lp)=='064ff3731955b515a07e12e3897102e2cd9d137ac91c43ec222e393a2e492d08'
    ledger=json.loads(lp.read_text());schema=ledger['matrices']['exact_markers']
    for name,pin in schema['files'].items():assert sha(lr/name)==pin
    counts=sparse.csr_matrix(tuple(np.load(lr/f'exact_markers-{k}.npy') for k in ['data','indices','indptr']),shape=tuple(schema['shape']))
    vr=root/'topic-qmsum-lexical-context-final-first-20261002';baseline=json.loads((vr/'results.json').read_text())
    assert sha(vr/'results.json')==ledger['baseline_sha256'] and sha(vr/'vocabulary.json')==baseline['vocabulary_sha256'] and sha(vr/'idf.npy')==baseline['idf_sha256']
    terms=json.loads((vr/'vocabulary.json').read_text());idf=np.load(vr/'idf.npy');positions={t:i for i,t in enumerate(terms)};del ledger,baseline
    excluded=frozenset(ENGLISH_STOP_WORDS)|{'yeah','okay','ok','uh','um','erm','hmm','mm','oh'}
    owners=np.full(counts.shape[0],-1,dtype=np.int64);source_meeting=np.full(len(owners),-1,dtype=np.int64);groups=[];label_rows={'block_labels':[],'message_labels':[]}
    for ordinal,(m,g,c) in enumerate(zip(meetings,geometry['records'],control['records'])):
        offset=g['source_offset'];source_meeting[offset:offset+len(m['meeting_transcripts'])]=ordinal
        block_group={b:len(groups)+i for i,group in enumerate(c['groups']) for b in group}
        for b in g['blocks']:
            ids=range(offset+b['start'],offset+b['end']+1)
            for i in ids:assert owners[i]==-1;owners[i]=block_group[b['ordinal']]
        for i,group in enumerate(c['groups']):
            groups.append({'id':len(groups),'meeting':ordinal,'blocks':group})
            for arm,key in [('block_labels','physical_block'),('message_labels','original_message')]:label_rows[arm].append({positions[t['term']]:1 for t in c['labels'][key][i]})
    assert groups==result['groups'] and (source_meeting>=0).all();group_meeting=np.array([g['meeting'] for g in groups]);pooled=[Counter() for _ in groups]
    feature_coordinates=0
    def normalize(row):
        values=[(j,(1+math.log(n))*float(idf[j])) for j,n in sorted(row.items())]
        norm=math.sqrt(math.fsum(v*v for j,v in values))
        return [(j,v/norm) for j,v in values] if norm else []
    def postings(rows):
        nonlocal feature_coordinates
        out=[([],[]) for _ in terms]
        for i,row in enumerate(rows):
            for j,value in normalize(row):out[j][0].append(i);out[j][1].append(value);feature_coordinates+=1
        return [(np.array(ids,dtype=np.int64),np.array(values)) for ids,values in out]
    def originals():
        for i in range(counts.shape[0]):
            row=counts[i];words={int(j):int(v) for j,v in zip(row.indices,row.data) if terms[j] not in excluded}
            if owners[i]>=0:pooled[owners[i]].update(words)
            yield words
    source_postings=postings(originals());banks={'group_words':postings(pooled)}
    banks.update({arm:postings(rows) for arm,rows in label_rows.items()})
    token=re.compile(r'(?u)\b\w\w+\b');marker=re.compile(r'\{(disfmarker|vocalsound)\}')
    def project(text):
        words=Counter(token.findall(text.lower()))
        for hit in marker.finditer(text):words[hit.group(1)]-=1
        assert all(n>=0 for n in words.values());words={t:n for t,n in words.items() if n and t not in excluded}
        known={positions[t]:n for t,n in words.items() if t in positions}
        return normalize(known),{'filtered_terms':len(words),'known_terms':len(known),'oov_terms':len(words)-len(known)}
    def scores(postings,n,query):
        out=np.zeros(n)
        for j,weight in query:
            ids,values=postings[j];out[ids]+=weight*values
        return out
    maximum=0.;rank_checks=near_choices=selected_scores=0;maximum_rank_gap=0.
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
    def check_rank(rows,values,eligible,limit):
        nonlocal rank_checks,near_choices,selected_scores,maximum_rank_gap
        rank_checks+=1;positive=eligible[values[eligible]>0]
        best=positive[np.lexsort((positive,-values[positive]))[:limit]]
        ids=[row['id'] for row in rows];assert len(ids)==len(best) and len(ids)==len(set(ids)) and set(ids)<=set(positive.tolist())
        assert rows==sorted(rows,key=lambda row:(-row['score'],row['id']))
        for chosen,expected in zip(rows,best):
            selected_scores+=1;close(float(values[chosen['id']]),chosen['score'])
            gap=float(values[expected]-values[chosen['id']]);maximum_rank_gap=max(maximum_rank_gap,gap)
            assert gap<=1e-12
            near_choices+=int(chosen['id']!=int(expected))
    arms=['direct_sources','group_words','block_labels','message_labels'];summary={scope:{arm:Counter() for arm in arms} for scope in ['meeting','global']}
    ready_ids=np.flatnonzero(owners>=0);group_ids=np.arange(len(groups),dtype=np.int64);support=Counter();record_ordinal=0
    for ordinal,(meeting,g) in enumerate(zip(meetings,geometry['records'])):
        offset=g['source_offset'];scopes={'meeting':(ready_ids[source_meeting[ready_ids]==ordinal],group_ids[group_meeting==ordinal]),'global':(ready_ids,group_ids)}
        for qordinal,q in enumerate(meeting['specific_query_list']):
            r=result['records'][record_ordinal];record_ordinal+=1
            assert (r['meeting'],r['query'])==(ordinal,qordinal) and r['query_sha256']==hashlib.sha256(q['query'].encode()).hexdigest()
            vector,metadata=project(q['query']);assert metadata==r['query_terms']
            wanted=sorted({offset+i for lo,hi in q['relevant_text_span'] for i in range(int(lo),int(hi)+1)})
            assert wanted==r['relevant_sources'] and r['spans']==[[int(lo),int(hi),lo,hi] for lo,hi in q['relevant_text_span']]
            ready=int((owners[wanted]>=0).sum());assert ready==r['ready_relevant_sources'];relevant=set(wanted)
            source_scores=scores(source_postings,len(owners),vector);group_scores={arm:scores(bank,len(groups),vector) for arm,bank in banks.items()}
            for scope,(ids,gids) in scopes.items():
                for arm in arms:
                    case=r['cases'][scope][arm]
                    if arm=='direct_sources':assert case['selected_groups']==[];candidates=ids
                    else:
                        check_rank(case['selected_groups'],group_scores[arm],gids,2)
                        chosen=[s['id'] for s in case['selected_groups']];candidates=ids[np.isin(owners[ids],chosen)]
                    assert case['candidate_sources']==len(candidates);check_rank(case['returned_sources'],source_scores,candidates,20)
                    returned=[s['id'] for s in case['returned_sources']];hits=[i+1 for i,j in enumerate(returned) if j in relevant]
                    dcg=sum(1/math.log2(i+1) for i in hits);ideal=sum(1/math.log2(i+1) for i in range(1,min(20,len(wanted))+1))
                    metric={'relevant_sources':len(wanted),'hits':len(hits),'recall_at_20':len(hits)/len(wanted),'precision_at_20':len(hits)/20,
                        'mrr':1/hits[0] if hits else 0.,'ndcg_at_20':dcg/ideal,'candidate_relevant_sources':len(relevant&set(candidates.tolist())),
                        'candidate_recall':len(relevant&set(candidates.tolist()))/len(wanted),'returned_sources':len(returned),'zero_results':int(not returned)}
                    close(metric,case['metrics']);summary[scope][arm].update(metric)
            support.update(queries=1,spans=len(q['relevant_text_span']),relevant_source_contributions=len(wanted),ready_relevant_source_contributions=ready,
                zero_known_term_queries=int(metadata['known_terms']==0))
        if ordinal%20==0:print(json.dumps({'audited_meetings':ordinal+1}),flush=True)
    assert record_ordinal==len(result['records'])==1095
    for scope,arms in summary.items():
        for arm,m in arms.items():summary[scope][arm]={'totals':dict(m),'macro':{k:m[k]/support['queries'] for k in ['recall_at_20','precision_at_20','mrr','ndcg_at_20','candidate_recall','returned_sources']}}
    close(summary,result['summary']);close(dict(support)|{'sources':len(owners),'ready_sources':len(ready_ids),'groups':len(groups)},result['support'])
    return {'capture_sha256':sha(path),'auditor_sha256':sha(Path(__file__)),'feature_coordinates_constructed':feature_coordinates,
        'queries_checked':record_ordinal,'retrieval_cases_checked':record_ordinal*8,'ranks_checked':rank_checks,'selected_scores_checked':selected_scores,
        'scalar_near_rank_choices':near_choices,'maximum_rank_gap':maximum_rank_gap,'maximum_numeric_error':maximum,
        'limitations':['shared original count/IDF/vocabulary and frozen audited label/tree capture, NumPy/SciPy/sklearn stoplist; tokenizer source ledger previously audited',
            'scalar fsum normalization may differ from scipy rounding: mathematical rank within1e-12, exact tie behavior separately fixed by fixtures/repeat',
            'original reference spans/metrics independently reconstructed; other meetings are unjudged, not known irrelevant',
            'not mash hybrid search, held-out coding/guided agent utility, semantic labels/ancestors, full ingestion/global lifecycle or Rust parity']}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--artifacts',type=Path,required=True);p.add_argument('--capture',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    result=audit(a.artifacts,a.capture)
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps(result),flush=True)


if __name__=='__main__':main()
