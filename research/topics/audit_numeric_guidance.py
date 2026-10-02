"""Independent original-count/query/span and rational-order numeric audit."""
import argparse
from collections import Counter
from fractions import Fraction
import json
import math
from pathlib import Path
import numpy as np
from scipy import sparse
from lexical_navigation_reference import load, digest, features, EXCLUDED, TOKEN, MARKER


REFERENCE = '1d23e6af3bc94373b47ebb3900eb913fe974a6a8efae9815528059f7428f1dd4'
ARMS = ['direct_sources','topic_parent','thread_parent']


def audit(root, path):
    data = json.loads(path.read_text()); rp = root/'topic-qmsum-thread-context-first-20261002.json'
    assert type(data['schema_version']) is int and data['schema_version'] == 1
    assert data['scope'] == 'candidate_numeric_guidance_training_not_production_selection'
    assert data['protocol'] == {'scale':10**12,'rrf_k':60,'new_inference':0,'llm_calls':0,'heldout_read':False,'production_selection':False}
    assert digest(rp) == REFERENCE == data['reference_capture_sha256']; old = json.loads(rp.read_text())
    ref = load(root, old['control_sha256'], data['source_sha256'])
    for name, pin in data['code_pins'].items(): assert digest(Path(__file__).with_name(name)) == pin
    lp = root/'topic-qmsum-label-cleanup-first-20261002/results.json'
    assert digest(lp) == '064ff3731955b515a07e12e3897102e2cd9d137ac91c43ec222e393a2e492d08'
    ledger = json.loads(lp.read_text()); schema = ledger['matrices']['exact_markers']
    for name, pin in schema['files'].items(): assert digest(lp.parent/name) == pin
    counts = sparse.csr_matrix(tuple(np.load(lp.parent/f'exact_markers-{k}.npy') for k in ['data','indices','indptr']),shape=tuple(schema['shape'])); del ledger
    clean = counts.multiply([t not in EXCLUDED for t in ref.terms]).tocsr(); clean.eliminate_zeros(); ready = np.flatnonzero(ref.owners >= 0)
    incidence = sparse.csr_matrix((np.ones(len(ready)),(ref.source_meeting[ready],ready)),shape=(len(ref.meetings),len(ref.owners)))
    threads, coordinates, error = features((incidence@clean).tocsr(),ref.idf)
    maximum = max(ref.maximum_scalar_error,error); selected = queries = 0
    gids = np.arange(len(ref.group_meeting)); tids = np.arange(len(ref.meetings)); positions = {t:i for i,t in enumerate(ref.terms)}
    summary = {s:{a:Counter() for a in ARMS} for s in ['meeting','global']}; changes = {s:{a:Counter() for a in ARMS} for s in summary}
    def check(left, right):
        nonlocal maximum
        if isinstance(left,dict):
            assert left.keys() == right.keys()
            for k in left: check(left[k],right[k])
        elif isinstance(left,list):
            assert len(left) == len(right)
            for x,y in zip(left,right): check(x,y)
        elif isinstance(left,float):
            maximum = max(maximum,abs(left-right)); assert math.isclose(left,right,rel_tol=1e-12,abs_tol=1e-12)
        else: assert left == right,(left,right)
    def ordered(scores, ids):
        return sorted([(int(i),float(scores[i]),round(Fraction(float(scores[i]))*10**12)) for i in ids if scores[i] > 0],key=lambda row:(-row[2],row[0]))
    def metric(rows, relevant, candidates):
        hits = [i+1 for i,row in enumerate(rows) if row['id'] in relevant]
        dcg = sum(1/math.log2(i+1) for i in hits); ideal = sum(1/math.log2(i+1) for i in range(1,min(20,len(relevant))+1))
        available = len(relevant & set(candidates.tolist()))
        return {'relevant_sources':len(relevant),'hits':len(hits),'recall_at_20':len(hits)/len(relevant),'precision_at_20':len(hits)/20,
            'mrr':1/hits[0] if hits else 0.,'ndcg_at_20':dcg/ideal,'candidate_relevant_sources':available,'candidate_recall':available/len(relevant),
            'returned_sources':len(rows),'zero_results':int(not rows)}
    assert len(data['records']) == len(old['records']) == 1095
    for row, previous in zip(data['records'],old['records']):
        assert {k:v for k,v in row.items() if k != 'cases'} == {k:v for k,v in previous.items() if k != 'cases'}
        meeting = row['meeting']; q = ref.meetings[meeting]['specific_query_list'][row['query']]; offset = ref.geometry['records'][meeting]['source_offset']
        relevant = {offset+i for lo,hi in q['relevant_text_span'] for i in range(int(lo),int(hi)+1)}; assert sorted(relevant) == row['relevant_sources']
        sources, topics, qerror = ref.scores(q['query']); maximum = max(maximum,qerror)
        words = Counter(TOKEN.findall(q['query'].lower()))
        for marker in MARKER.finditer(q['query']): words[marker.group(1)] -= 1
        known = sorted((positions[t],n) for t,n in words.items() if n and t not in EXCLUDED and t in positions)
        query_counts = sparse.csr_matrix(([n for j,n in known],([0]*len(known),[j for j,n in known])),shape=(1,len(ref.terms)))
        direction,_,qerror = features(query_counts,ref.idf); maximum = max(maximum,qerror); thread_scores = (threads@direction.T).toarray().ravel()
        scopes = {'meeting':(ready[ref.source_meeting[ready] == meeting],gids[ref.group_meeting == meeting],np.array([meeting])), 'global':(ready,gids,tids)}
        assert set(row['cases']) == set(scopes)
        for scope,(ids,pids,mids) in scopes.items():
            order = ordered(sources,ids); cases = row['cases'][scope]['numeric']
            assert set(cases) == set(ARMS) and cases['direct_sources']['selected_groups'] == []
            direct = [{'id':i,'score':v,'source_key':key,'source_rank':r} for r,(i,v,key) in enumerate(order[:20],1)]
            assert cases['direct_sources']['candidate_sources'] == len(ids) and cases['direct_sources']['positive_sources'] == len(order)
            check(direct,cases['direct_sources']['returned_sources']); wanted = {'direct_sources':direct}; drift = {}
            for arm,owners,scores,parent_ids in [('topic_parent',ref.owners,topics,pids),('thread_parent',ref.source_meeting,thread_scores,mids)]:
                parents = ordered(scores,parent_ids); ranks = {i:(r,v,key) for r,(i,v,key) in enumerate(parents,1)}; all_rows = []
                for r,(i,value,key) in enumerate(order,1):
                    parent = int(owners[i]); pr,pv,pk = ranks.get(parent,(None,0.,None)); weight = Fraction(1,60+r)+(Fraction(1,60+pr) if pr else 0)
                    result = {'id':i,'score':float(weight),'score_numerator':weight.numerator,'score_denominator':weight.denominator,
                        'source_score':value,'source_key':key,'source_rank':r,'parent_group':parent,'parent_rank':pr,'parent_score':pv,'parent_key':pk}
                    all_rows.append((weight,i,result))
                all_rows.sort(key=lambda r:(-r[0],r[1])); chosen = [r[2] for r in all_rows[:20]]
                check(chosen,cases[arm]['returned_sources']); assert cases[arm]['candidate_sources'] == len(order) and cases[arm]['positive_groups'] == len(parents)
                wanted[arm] = chosen
            if scope == 'meeting': assert [r['id'] for r in wanted['thread_parent']] == [r['id'] for r in direct]
            for arm, rows in wanted.items():
                m = metric(rows,relevant,ids); check(m,cases[arm]['metrics']); summary[scope][arm].update(m); selected += len(rows)
                left = [r['id'] for r in previous['cases'][scope][arm]['returned_sources']]; right = [r['id'] for r in rows]
                delta = m['recall_at_20']-previous['cases'][scope][arm]['metrics']['recall_at_20']
                drift[arm] = {'order_changed':left != right,'removed':sorted(set(left)-set(right)),'added':sorted(set(right)-set(left)),
                    'changed_positions':sum(a != b for a,b in zip(left,right)),'recall_delta':delta}
                changes[scope][arm].update(order_changed=int(left != right),membership_changed=int(set(left) != set(right)),
                    improved=int(delta > 0),declined=int(delta < 0),tied=int(delta == 0))
            check(drift,row['cases'][scope]['drift'])
        queries += 1
        if queries%50 == 0: print(json.dumps({'audited_queries':queries}),flush=True)
    for scope, arms in summary.items():
        for arm,m in arms.items(): summary[scope][arm] = {'totals':dict(m),'macro':{k:m[k]/queries for k in
            ['recall_at_20','precision_at_20','mrr','ndcg_at_20','candidate_recall','returned_sources']}}
    check(summary,data['summary']); check(changes,data['changes']); check(data['support'],old['support'])
    return {'capture_sha256':digest(path),'auditor_sha256':digest(Path(__file__)),'reference_sha256':digest(Path(__file__).with_name('lexical_navigation_reference.py')),
        'queries_checked':queries,'retrieval_cases_checked':queries*6,'selected_sources_checked':selected,'feature_coordinates_checked':ref.coordinates+coordinates,
        'maximum_scalar_feature_error':max(ref.maximum_scalar_error,error),'maximum_numeric_error':maximum,
        'limitations':['original count/query/span/ownership/formula/metric reconstruction without producer imports;Fraction conversion/arithmetic shared',
            'exact score projection replay shares NumPy/SciPy IEEE kernels;not cross-platform score or Rust parity',
            'frozen counts/IDF/topic inventory/sklearn shared;training not semantic/coding/held-out/agent utility or online/full-ingestion evidence']}


def main():
    p = argparse.ArgumentParser(description=__doc__); p.add_argument('--artifacts',type=Path,required=True)
    p.add_argument('--capture',type=Path,required=True); p.add_argument('--output',type=Path,required=True); a = p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]): raise ValueError('fresh outside-Git output required')
    result = audit(a.artifacts,a.capture)
    with a.output.open('x') as f: json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps(result),flush=True)


if __name__ == '__main__': main()
