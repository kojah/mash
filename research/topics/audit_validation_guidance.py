"""Independent held-out original-count, hierarchy, retrieval and bootstrap audit.

No experiment producer imports. Compiled linkage/IEEE kernels, frozen source
unit ledger and training word frame are shared; this is not Rust parity.
"""
import argparse
from collections import Counter
from fractions import Fraction
import json
import math
from pathlib import Path
import numpy as np
from scipy import sparse
from scipy.cluster.hierarchy import linkage
from scipy.spatial.distance import squareform
from lexical_navigation_reference import digest, features, EXCLUDED, TOKEN, MARKER


def words(text):
    result = Counter(TOKEN.findall(text.lower()))
    for marker in MARKER.finditer(text): result[marker.group(1)] -= 1
    assert all(n >= 0 for n in result.values())
    return {t:n for t,n in sorted(result.items()) if n}


def audit(root, path):
    data = json.loads(path.read_text()); maximum = 0.; coordinates = 0
    def check(left, right):
        nonlocal maximum
        if isinstance(left, dict):
            assert left.keys() == right.keys(), (left.keys(), right.keys())
            for k in left: check(left[k], right[k])
        elif isinstance(left, list):
            assert len(left) == len(right)
            for x,y in zip(left,right): check(x,y)
        elif isinstance(left, float):
            maximum = max(maximum,abs(left-right))
            assert math.isclose(left,right,rel_tol=1e-12,abs_tol=1e-12), (left,right)
        else: assert left == right, (left,right)
    assert data['scope'] == 'frozen_author_validation_guidance_not_production_selection'
    for name,pin in data['code_pins'].items(): assert digest(Path(__file__).with_name(name)) == pin
    source = root/'topic-qmsum-author-validation-20261002'
    sp = source/'data/ALL/jsonl/val.jsonl'; assert digest(sp) == data['source_sha256']
    assert digest(source/'capture.json') == data['capture_sha256']
    assert digest(Path(__file__).with_name('GUIDANCE_VALIDATION_PROTOCOL.md')) == data['protocol_sha256']
    meetings = [json.loads(line) for line in sp.read_bytes().splitlines()]
    up = root/'qmsum-validation-units-first-20261002.json'
    assert digest(up) == data['inputs']['units_sha256']; units = json.loads(up.read_text())
    assert units['source_sha256'] == data['source_sha256']
    folder = root/'topic-qmsum-lexical-context-final-first-20261002'
    for name,key in [('results.json','training_frame_sha256'),('vocabulary.json','vocabulary_sha256'),('idf.npy','idf_sha256')]:
        assert digest(folder/name) == data['inputs'][key]
    terms = json.loads((folder/'vocabulary.json').read_text()); idf = np.load(folder/'idf.npy')
    positions = {t:i for i,t in enumerate(terms)}; rows = []; cols = []; values = []; offset = 0; support = Counter()
    offsets = []
    for meeting in meetings:
        offsets.append(offset)
        for turn in meeting['meeting_transcripts']:
            raw = words(turn['content']); known = sorted((positions[t],n) for t,n in raw.items() if t in positions)
            rows.extend([offset]*len(known)); cols.extend(j for j,n in known); values.extend(n for j,n in known)
            support.update(source_unique_terms=len(raw),source_oov_unique_terms=sum(t not in positions for t in raw),
                source_oov_occurrences=sum(n for t,n in raw.items() if t not in positions),source_zero_known=int(not known))
            offset += 1
    counts = sparse.csr_matrix((values,(rows,cols)),shape=(offset,len(terms)),dtype=float); counts.sort_indices()
    ready = np.array([r['ready'] for r in units['rows']],dtype=bool)
    owners = np.full(offset,-1,dtype=np.int64); sm = np.full(offset,-1,dtype=np.int64); gm = []
    for m,(meeting,u,record,start) in enumerate(zip(meetings,units['meetings'],data['inventory'],offsets)):
        n = len(meeting['meeting_transcripts']); local = np.array(u['owners']); blocks = u['blocks']; size = len(blocks)
        assert u['source_offset'] == start and np.array_equal(local >= 0,ready[start:start+n])
        # Independently map the sealed original bounds; no producer tree/owner helper.
        expected = np.full(n,-1,dtype=np.int64)
        for b in blocks:
            assert (expected[b['start']:b['end']+1] == -1).all()
            expected[b['start']:b['end']+1] = b['ordinal']
        assert np.array_equal(local,expected); sm[start:start+n] = m
        ids = np.flatnonzero(local >= 0)
        incidence = sparse.csr_matrix((np.ones(len(ids)),(local[ids],ids)),shape=(size,n))
        bank,number,error = features((incidence@counts[start:start+n]).tocsr(),idf); coordinates += number; maximum = max(maximum,error)
        similarity = (bank@bank.T).toarray(); assert np.array_equal(similarity,similarity.T)
        distance = 1-np.clip(similarity,-1,1); np.fill_diagonal(distance,0)
        merges = linkage(squareform(distance),method='average').tolist() if size > 1 else []
        children = {size+i:(int(row[0]),int(row[1])) for i,row in enumerate(merges)}
        members = {i:[i] for i in range(size)}
        for node,(left,right) in children.items(): members[node] = sorted(members[left]+members[right])
        selected = [2*size-2] if size else []
        while len(selected) < min(8,size):
            split = min((i for i in selected if i in children),key=lambda i:(-len(members[i]),members[i]))
            selected.remove(split); selected.extend(children[split])
        groups = []
        for member_ids in sorted(members[i] for i in selected):
            gid = len(gm); gm.append(m); chosen = np.flatnonzero(np.isin(local,member_ids))+start; owners[chosen] = gid
            groups.append({'id':gid,'blocks':member_ids,'original_ready_sources':chosen.tolist()})
        tree = {'root':2*size-2,'children':{str(k):list(v) for k,v in children.items()},'leaves':{str(i):i for i in range(size)}} if size else None
        check({'meeting':m,'source_offset':start,'tree':tree,'merges':merges,'groups':groups,
               'zero_raw_lexical_blocks':int((bank.getnnz(axis=1) == 0).sum())},record)
    assert len(meetings) == 35 and offset == 20123 and np.array_equal(owners >= 0,ready)
    clean = counts.multiply([t not in EXCLUDED for t in terms]).tocsr(); clean.eliminate_zeros()
    sources,number,error = features(clean,idf); coordinates += number; maximum = max(maximum,error)
    ids = np.flatnonzero(ready); banks = []
    for owner,size in [(owners,len(gm)),(sm,len(meetings))]:
        incidence = sparse.csr_matrix((np.ones(len(ids)),(owner[ids],ids)),shape=(size,offset))
        bank,number,error = features((incidence@clean).tocsr(),idf); coordinates += number; maximum = max(maximum,error); banks.append(bank)
    support.update(sources=offset,ready=len(ids),unsupported=offset-len(ids),blocks=len(units['meetings']) and sum(len(m['blocks']) for m in units['meetings']),
                   topic_parents=len(gm),thread_parents=len(meetings))
    gm = np.array(gm); gids = np.arange(len(gm)); tids = np.arange(len(meetings))
    def order(scores, ids, numeric):
        rows = [(int(i),float(scores[i])) for i in ids if scores[i] > 0]
        return sorted(rows,key=lambda r:(-round(Fraction(r[1])*10**12) if numeric else -r[1],r[0]))
    def metric(returned,relevant,candidates):
        hits = [i+1 for i,row in enumerate(returned) if row['id'] in relevant]
        dcg = sum(1/math.log2(i+1) for i in hits); ideal = sum(1/math.log2(i+1) for i in range(1,min(20,len(relevant))+1))
        available = len(relevant & set(candidates.tolist()))
        return {'relevant_sources':len(relevant),'hits':len(hits),'recall_at_20':len(hits)/len(relevant),'precision_at_20':len(hits)/20,
            'mrr':1/hits[0] if hits else 0.,'ndcg_at_20':dcg/ideal,'candidate_relevant_sources':available,
            'candidate_recall':available/len(relevant),'returned_sources':len(returned),'zero_results':int(not returned)}
    summary = {s:{a:Counter() for a in data['summary'][s]} for s in ['meeting','global']}
    records = iter(data['records']); queries = selected_sources = 0; paired = {'primary_recall':[],'mrr_safety':[]}; query_meetings = []
    for m,meeting in enumerate(meetings):
        for qid,q in enumerate(meeting['specific_query_list']):
            row = next(records); assert row['meeting'] == m and row['query'] == qid
            check(q['relevant_text_span'],row['original_spans'])
            relevant = {offsets[m]+i for lo,hi in q['relevant_text_span'] for i in range(int(lo),int(hi)+1)}
            check(sorted(relevant),row['relevant_sources'])
            raw = {t:n for t,n in words(q['query']).items() if t not in EXCLUDED}; known = sorted((positions[t],n) for t,n in raw.items() if t in positions)
            metadata = {'filtered_terms':len(raw),'known_terms':len(known),'oov_terms':len(raw)-len(known)}; check(metadata,row['query_terms'])
            support.update(queries=1,query_zero_known=int(not known),query_oov_terms=metadata['oov_terms'])
            qc = sparse.csr_matrix(([n for j,n in known],([0]*len(known),[j for j,n in known])),shape=(1,len(terms)))
            direction,number,error = features(qc,idf); coordinates += number; maximum = max(maximum,error)
            scores = (sources@direction.T).toarray().ravel(); ps = [(b@direction.T).toarray().ravel() for b in banks]
            scopes = {'meeting':(ids[sm[ids] == m],gids[gm == m],np.array([m])), 'global':(ids,gids,tids)}
            for scope,(sids,pids,mids) in scopes.items():
                for arm,capture in row['cases'][scope].items():
                    numeric = arm.startswith('numeric'); ranked = order(scores,sids,numeric)
                    if arm.endswith('direct'):
                        chosen = [{'id':i,'score':value} | ({'source_key':round(Fraction(value)*10**12),'source_rank':rank} if numeric else {})
                                  for rank,(i,value) in enumerate(ranked[:20],1)]
                        expected = {'candidate_sources':len(sids),'returned_sources':chosen}
                        if numeric: expected['positive_sources'] = len(ranked)
                    else:
                        topic = arm.endswith('topic'); owner = owners if topic else sm; parent_scores = ps[0 if topic else 1]
                        parents = order(parent_scores,pids if topic else mids,numeric); ranks = {i:(rank,value) for rank,(i,value) in enumerate(parents,1)}
                        fused = []
                        for rank,(i,value) in enumerate(ranked,1):
                            parent = int(owner[i]); pr,pv = ranks.get(parent,(None,0.))
                            weight = Fraction(1,60+rank)+(Fraction(1,60+pr) if pr else 0) if numeric else 1/(60+rank)+(1/(60+pr) if pr else 0.)
                            result = {'id':i,'score':float(weight),'source_score':value,'source_rank':rank,'parent_group':parent,'parent_rank':pr,'parent_score':pv}
                            if numeric: result.update(score_numerator=weight.numerator,score_denominator=weight.denominator,
                                source_key=round(Fraction(value)*10**12),parent_key=round(Fraction(pv)*10**12) if pr else None)
                            fused.append((weight,i,result))
                        fused.sort(key=lambda r:(-r[0],r[1])); chosen = [r[2] for r in fused[:20]]
                        expected = {'candidate_sources':len(ranked),'positive_groups':len(parents),'returned_sources':chosen}
                    expected['metrics'] = metric(chosen,relevant,sids); check(expected,capture)
                    summary[scope][arm].update(expected['metrics']); selected_sources += len(chosen)
            for name,field in [('primary_recall','recall_at_20'),('mrr_safety','mrr')]:
                paired[name].append(row['cases']['meeting']['exact_topic']['metrics'][field]-row['cases']['meeting']['exact_direct']['metrics'][field])
            query_meetings.append(m); queries += 1
        print(json.dumps({'audited_meetings':m+1,'queries':queries}),flush=True)
    assert next(records,None) is None and queries == 237
    for scope,arms in summary.items():
        for arm,totals in arms.items(): summary[scope][arm] = {'totals':dict(totals),'macro':{k:totals[k]/queries for k in
            ['recall_at_20','precision_at_20','mrr','ndcg_at_20','candidate_recall','returned_sources']}}
    check(summary,data['summary']); check(dict(support),data['support'])
    # Expanded selected query oracle, rather than producer bincount reductions.
    draws = np.random.Generator(np.random.PCG64(20261002)).integers(0,35,size=(10000,35)); bootstrap = {}
    for name,values in paired.items():
        grouped = [[v for m,v in zip(query_meetings,values) if m == j] for j in range(35)]
        means = [math.fsum(v for m in draw for v in grouped[m])/sum(len(grouped[m]) for m in draw) for draw in draws]
        lower,upper = np.percentile(means,[2.5,97.5])
        bootstrap[name] = {'point':math.fsum(values)/queries,'lower95':float(lower),'upper95':float(upper),
                           'resamples':10000,'seed':20261002,'meetings':35,'queries':queries}
    check(bootstrap,data['bootstrap']); check({'primary_recall':bootstrap['primary_recall']['lower95'] > 0,
                                            'mrr_safety':bootstrap['mrr_safety']['lower95'] > -.02},data['gates'])
    assert digest(sp) == data['source_sha256'] and digest(up) == data['inputs']['units_sha256']
    return {'capture_sha256':digest(path),'auditor_sha256':digest(Path(__file__)),
        'reference_sha256':digest(Path(__file__).with_name('lexical_navigation_reference.py')),
        'additional_dependency_sha256':{'captured_hierarchy.py':digest(Path(__file__).with_name('captured_hierarchy.py'))},
        'meetings_checked':35,'queries_checked':queries,'cases_checked':queries*12,'selected_sources_checked':selected_sources,
        'feature_coordinates_checked':coordinates,'maximum_numeric_error':maximum,'bootstrap':bootstrap,'gates':data['gates'],
        'limitations':['no producer imports; original text counts, hierarchy/cut/ownership, queries/spans, all ranks/metrics and expanded-query bootstrap reconstructed',
            'sealed original unit readiness/bounds already independently audited; training frame/stoplist, tokenizer assets, SciPy linkage and NumPy/SciPy IEEE kernels shared',
            'scalar feature tolerance is not scalar rank winner or cross-platform/Rust parity; Fraction and bootstrap generator/percentile shared',
            'held-out meeting relevance is not coding label/multitag/ancestor/agent quality or bounded online/full-ingestion evidence']}


def main():
    p = argparse.ArgumentParser(description=__doc__); p.add_argument('--artifacts',type=Path,required=True)
    p.add_argument('--capture',type=Path,required=True); p.add_argument('--output',type=Path,required=True); a = p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]): raise ValueError('fresh outside-Git output required')
    result = audit(a.artifacts,a.capture)
    with a.output.open('x') as f: json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps(result),flush=True)


if __name__ == '__main__': main()
