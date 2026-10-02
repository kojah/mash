"""Paired complete-query numerical drift/cost control; no production adoption."""
import argparse
from collections import Counter
import json
from pathlib import Path
import resource
import time
import numpy as np
from discovery_inputs import digest
from guidance_corpus import load
from navigation_features import build
from query_navigation import query, rank, metrics
from vector_topic_guidance import retrieve as exact
from numeric_guidance import ranked, retrieve


REFERENCE = '1d23e6af3bc94373b47ebb3900eb913fe974a6a8efae9815528059f7428f1dd4'
PINS = ['qmsum_numeric_guidance.py', 'NUMERIC_GUIDANCE_PROTOCOL.md', 'numeric_guidance.py',
        'guidance_corpus.py', 'navigation_features.py', 'vector_topic_guidance.py',
        'query_navigation.py', 'discovery_inputs.py', 'episode_blocks.py', 'label_noise.py',
        'stable_lexical_frame.py', 'qmsum.py', 'uv.lock']
ARMS = ['direct_sources', 'topic_parent', 'thread_parent']


def payload(result):
    return {'candidate_sources': result.candidate_sources, 'positive_groups': result.positive_parents,
            'returned_sources': [{'id': row.source.id, 'score': float(row.score),
                'score_numerator': row.score.numerator, 'score_denominator': row.score.denominator,
                'source_score': row.source.score, 'source_key': row.source.key, 'source_rank': row.source.rank,
                'parent_group': row.parent_id, 'parent_rank': row.parent.rank if row.parent else None,
                'parent_score': row.parent.score if row.parent else 0.,
                'parent_key': row.parent.key if row.parent else None} for row in result.matches]}


def experiment(root):
    start = time.perf_counter(); cpu = time.process_time()
    pins = {n: digest(Path(__file__).with_name(n)) for n in PINS}
    path = root/'topic-qmsum-thread-context-first-20261002.json'
    if digest(path) != REFERENCE: raise ValueError('frozen thread control required')
    previous = json.loads(path.read_text()); c = load(root); data = c.data
    begin = time.perf_counter()
    topics = build(data.counts, data.idf, data.terms, c.topic_owner, len(c.topic_meeting))
    threads = build(data.counts, data.idf, data.terms, np.where(data.ready, c.source_meeting, -1), len(data.meetings))
    if (topics.sources != threads.sources).nnz: raise ValueError('source frame drift')
    features_ms = (time.perf_counter()-begin)*1000
    setup_ms = (time.perf_counter()-start)*1000
    positions = {t:i for i,t in enumerate(data.terms)}
    ids = np.flatnonzero(data.ready); gids = np.arange(len(c.topic_meeting)); tids = np.arange(len(data.meetings))
    records = []; times = []
    summary = {s:{a:Counter() for a in ARMS} for s in ['meeting','global']}
    changes = {s:{a:Counter() for a in ARMS} for s in summary}
    for old in previous['records']:
        meeting = data.meetings[old['meeting']]; q = meeting.specific_queries[old['query']]
        begin = time.perf_counter(); direction, metadata = query(q.query, positions, data.idf); direction.sort_indices()
        scores, group_scores = topics.score(direction); thread_scores = (threads.groups@direction.T).toarray().ravel()
        projection_ms = (time.perf_counter()-begin)*1000
        if metadata != old['query_terms']: raise ValueError('query metadata drift')
        scopes = {'meeting': (ids[c.source_meeting[ids] == meeting.ordinal], gids[c.topic_meeting == meeting.ordinal], np.array([meeting.ordinal])),
                  'global': (ids, gids, tids)}
        cases = {}; stages = {}
        for scope, (sids, pids, mids) in scopes.items():
            begin = time.perf_counter()
            replay = {'direct_sources': {'selected_groups': [], 'returned_sources': rank(scores[sids], sids, 20), 'candidate_sources': len(sids)},
                'topic_parent': exact(scores[sids], sids, c.topic_owner[sids], group_scores[pids], pids),
                'thread_parent': exact(scores[sids], sids, c.source_meeting[sids], thread_scores[mids], mids)}
            for arm, result in replay.items():
                result['metrics'] = metrics(result, old['relevant_sources'], sids.tolist())
                if result != old['cases'][scope][arm]: raise ValueError('exact source/parent replay drift')
            replay_ms = (time.perf_counter()-begin)*1000
            source_values = scores[sids].tolist(); source_ids = sids.tolist()
            begin = time.perf_counter(); ordered = ranked(source_values, source_ids)
            direct = {'selected_groups': [], 'candidate_sources': len(sids), 'positive_sources': len(ordered),
                      'returned_sources': [{'id': r.id, 'score': r.score, 'source_key': r.key, 'source_rank': r.rank} for r in ordered[:20]]}
            direct['metrics'] = metrics(direct, old['relevant_sources'], source_ids)
            direct_ms = (time.perf_counter()-begin)*1000
            begin = time.perf_counter()
            topic = payload(retrieve(source_values, source_ids, c.topic_owner[sids].tolist(), group_scores[pids].tolist(), pids.tolist()))
            topic['metrics'] = metrics(topic, old['relevant_sources'], source_ids); topic_ms = (time.perf_counter()-begin)*1000
            begin = time.perf_counter()
            thread = payload(retrieve(source_values, source_ids, c.source_meeting[sids].tolist(), thread_scores[mids].tolist(), mids.tolist()))
            thread['metrics'] = metrics(thread, old['relevant_sources'], source_ids); thread_ms = (time.perf_counter()-begin)*1000
            new = {'direct_sources': direct, 'topic_parent': topic, 'thread_parent': thread}; drift = {}
            if scope == 'meeting' and ([r['id'] for r in thread['returned_sources']] != [r['id'] for r in direct['returned_sources']] or thread['metrics'] != direct['metrics']):
                raise ValueError('sole-parent quantized direct invariant failed')
            for arm, result in new.items():
                before = old['cases'][scope][arm]; left = [r['id'] for r in before['returned_sources']]; right = [r['id'] for r in result['returned_sources']]
                if len(left) != len(right) or result['metrics']['candidate_recall'] != before['metrics']['candidate_recall']:
                    raise ValueError('eligibility/result budget drift')
                delta = result['metrics']['recall_at_20']-before['metrics']['recall_at_20']
                drift[arm] = {'order_changed': left != right, 'removed': sorted(set(left)-set(right)), 'added': sorted(set(right)-set(left)),
                             'changed_positions': sum(a != b for a,b in zip(left,right)), 'recall_delta': delta}
                changes[scope][arm].update(order_changed=int(left != right), membership_changed=int(set(left) != set(right)),
                    improved=int(delta > 0), declined=int(delta < 0), tied=int(delta == 0))
                summary[scope][arm].update(result['metrics'])
            cases[scope] = {'numeric': new, 'drift': drift}
            stages[scope] = {'exact_replay_ms': replay_ms, 'direct_ms': direct_ms, 'topic_ms': topic_ms, 'thread_ms': thread_ms}
        records.append({k:v for k,v in old.items() if k != 'cases'} | {'cases': cases})
        times.append({'meeting': meeting.ordinal, 'query': q.ordinal, 'projection_ms': projection_ms, 'scopes': stages})
        if len(records)%50 == 0: print(json.dumps({'processed_queries': len(records), 'total_queries':1095}), flush=True)
    if len(records) != 1095 or digest(path) != REFERENCE or pins != {n:digest(Path(__file__).with_name(n)) for n in PINS}:
        raise ValueError('complete source/code pins changed')
    for scope, arms in summary.items():
        for arm, m in arms.items(): summary[scope][arm] = {'totals':dict(m), 'macro':{k:m[k]/len(records) for k in
            ['recall_at_20','precision_at_20','mrr','ndcg_at_20','candidate_recall','returned_sources']}}
    return {'schema_version':1, 'scope':'candidate_numeric_guidance_training_not_production_selection',
        'source_sha256':data.source, 'reference_capture_sha256':REFERENCE, 'support':previous['support'],
        'code_pins':pins, 'summary':summary, 'changes':changes, 'records':records,
        'protocol':{'scale':10**12,'rrf_k':60,'new_inference':0,'llm_calls':0,'heldout_read':False,'production_selection':False},
        'timings':{'setup_ms':setup_ms,'features_ms':features_ms,'after_imports_ms':(time.perf_counter()-start)*1000,
            'cpu_ms':(time.process_time()-cpu)*1000,'rusage_peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,'queries':times}}


def main():
    p = argparse.ArgumentParser(description=__doc__); p.add_argument('--artifacts',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True); p.add_argument('--compare',type=Path); a = p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]): raise ValueError('fresh outside-Git output required')
    result = experiment(a.artifacts)
    if a.compare and {k:v for k,v in json.loads(a.compare.read_text()).items() if k != 'timings'} != {k:v for k,v in result.items() if k != 'timings'}:
        raise ValueError('complete non-timing repeat differs')
    with a.output.open('x') as f: json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps({'summary':result['summary'],'changes':result['changes']}),flush=True)


if __name__ == '__main__': main()
