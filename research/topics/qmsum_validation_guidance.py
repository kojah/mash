"""Frozen complete-query held-out guidance evaluation; no method tuning."""
import argparse
from collections import Counter
import json
from pathlib import Path
import resource
import time
import numpy as np
from validation_inventory import build, digest
from navigation_features import build as frame
from query_navigation import query, rank, metrics
from vector_topic_guidance import retrieve as exact
from numeric_guidance import retrieve as numeric, ranked
from qmsum_numeric_guidance import payload
from validation_bootstrap import paired


ARMS = ['exact_direct','exact_topic','exact_thread','numeric_direct','numeric_topic','numeric_thread']
PINS = ['qmsum_validation_guidance.py','validation_inventory.py','validation_bootstrap.py','GUIDANCE_VALIDATION_PROTOCOL.md',
    'navigation_features.py','query_navigation.py','vector_topic_guidance.py','numeric_guidance.py','qmsum_numeric_guidance.py',
    'qmsum_validation_capture.py','qmsum.py','episode_blocks.py','stable_frame_quality.py','block_hierarchy.py',
    'stable_lexical_frame.py','label_noise.py','uv.lock']


def experiment(root):
    start = time.perf_counter();cpu = time.process_time();pins = {n:digest(Path(__file__).with_name(n)) for n in PINS}
    inventory = build(root);source = inventory.source;begin = time.perf_counter()
    topics = frame(inventory.counts,inventory.idf,inventory.terms,inventory.topic_owner,len(inventory.topic_meeting))
    threads = frame(inventory.counts,inventory.idf,inventory.terms,np.where(inventory.ready,inventory.source_meeting,-1),len(source.meetings))
    if (topics.sources != threads.sources).nnz:raise ValueError('source frame differs by parent control')
    feature_ms = (time.perf_counter()-begin)*1000;setup_ms = (time.perf_counter()-start)*1000
    # Inventory is complete before native queries/spans enter evaluation.
    positions = {t:i for i,t in enumerate(inventory.terms)};ids = np.flatnonzero(inventory.ready)
    gids = np.arange(len(inventory.topic_meeting),dtype=np.int64);tids = np.arange(len(source.meetings),dtype=np.int64)
    summary = {s:{a:Counter() for a in ARMS} for s in ['meeting','global']};records = [];times = [];qstats = Counter()
    for meeting in source.meetings:
        offset = inventory.records[meeting.ordinal]['source_offset']
        for q in meeting.specific_queries:
            relevant = sorted({offset+i for span in q.spans for i in range(span.start,span.end+1)})
            begin = time.perf_counter();direction,metadata = query(q.query,positions,inventory.idf);direction.sort_indices()
            scores,groups = topics.score(direction);thread_scores = (threads.groups@direction.T).toarray().ravel();projection_ms = (time.perf_counter()-begin)*1000
            qstats.update(queries=1,query_zero_known=int(metadata['known_terms'] == 0),query_oov_terms=metadata['oov_terms'])
            scopes = {'meeting':(ids[inventory.source_meeting[ids] == meeting.ordinal],gids[inventory.topic_meeting == meeting.ordinal],np.array([meeting.ordinal],dtype=np.int64)),
                      'global':(ids,gids,tids)};cases = {};stages = {}
            for scope,(sids,pids,mids) in scopes.items():
                cases[scope] = {};stages[scope] = {}
                source_values = scores[sids].tolist();source_ids = sids.tolist()
                for arm in ARMS:
                    begin = time.perf_counter()
                    if arm == 'exact_direct':result = {'candidate_sources':len(sids),'returned_sources':rank(scores[sids],sids,20)}
                    elif arm == 'numeric_direct':
                        ordered = ranked(source_values,source_ids);result = {'candidate_sources':len(sids),'positive_sources':len(ordered),
                            'returned_sources':[{'id':r.id,'score':r.score,'source_key':r.key,'source_rank':r.rank} for r in ordered[:20]]}
                    else:
                        is_topic = arm.endswith('topic');owners = inventory.topic_owner if is_topic else inventory.source_meeting
                        parent_scores = groups if is_topic else thread_scores;parent_ids = pids if is_topic else mids
                        result = exact(scores[sids],sids,owners[sids],parent_scores[parent_ids],parent_ids) if arm.startswith('exact') else payload(
                            numeric(source_values,source_ids,owners[sids].tolist(),parent_scores[parent_ids].tolist(),parent_ids.tolist()))
                    result['metrics'] = metrics(result,relevant,source_ids);stages[scope][arm+'_ms'] = (time.perf_counter()-begin)*1000
                    cases[scope][arm] = result;summary[scope][arm].update(result['metrics'])
                if scope == 'meeting':
                    for prefix in ['exact','numeric']:
                        left = cases[scope][prefix+'_direct'];right = cases[scope][prefix+'_thread']
                        if [r['id'] for r in left['returned_sources']] != [r['id'] for r in right['returned_sources']] or left['metrics'] != right['metrics']:
                            raise ValueError('known-meeting sole-parent direct invariant failed')
            records.append({'meeting':meeting.ordinal,'query':q.ordinal,'query_terms':metadata,'relevant_sources':relevant,
                            'original_spans':[[s.original_start,s.original_end] for s in q.spans],'cases':cases})
            times.append({'meeting':meeting.ordinal,'query':q.ordinal,'projection_ms':projection_ms,'scopes':stages})
        print(json.dumps({'processed_meetings':meeting.ordinal+1,'queries':len(records)}),flush=True)
    if len(records) != 237 or pins != {n:digest(Path(__file__).with_name(n)) for n in PINS}:raise ValueError('complete source/code support drift')
    for scope,arms in summary.items():
        for arm,m in arms.items():summary[scope][arm] = {'totals':dict(m),'macro':{k:m[k]/len(records) for k in
            ['recall_at_20','precision_at_20','mrr','ndcg_at_20','candidate_recall','returned_sources']}}
    bootstrap = {}
    for name,metric in [('primary_recall','recall_at_20'),('mrr_safety','mrr')]:
        bootstrap[name] = paired([r['meeting'] for r in records],
            [r['cases']['meeting']['exact_topic']['metrics'][metric]-r['cases']['meeting']['exact_direct']['metrics'][metric] for r in records],35)
    return {'schema_version':1,'scope':'frozen_author_validation_guidance_not_production_selection','source_sha256':source.source_sha256,
        'capture_sha256':source.capture_sha256,'protocol_sha256':source.protocol_sha256,'inputs':inventory.inputs,'code_pins':pins,
        'inventory':inventory.records,'support':inventory.support | dict(qstats),'summary':summary,'records':records,'bootstrap':bootstrap,
        'gates':{'primary_recall':bootstrap['primary_recall']['lower95'] > 0,'mrr_safety':bootstrap['mrr_safety']['lower95'] > -.02},
        'protocol':{'new_inference':0,'llm_calls':0,'heldout_used_to_fit':False,'production_selection':False,
                    'geometry':'original_raw_exact_marker_counts_training_idf','search':'original_static_noise_mask'},
        'timings':{'setup_ms':setup_ms,'features_ms':feature_ms,'after_imports_ms':(time.perf_counter()-start)*1000,
            'cpu_ms':(time.process_time()-cpu)*1000,'rusage_peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,'queries':times}}


def main():
    p = argparse.ArgumentParser(description=__doc__);p.add_argument('--artifacts',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True);p.add_argument('--compare',type=Path);a = p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    result = experiment(a.artifacts)
    if a.compare and {k:v for k,v in json.loads(a.compare.read_text()).items() if k != 'timings'} != {k:v for k,v in result.items() if k != 'timings'}:
        raise ValueError('complete held-out repeat differs')
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps({'summary':result['summary'],'bootstrap':result['bootstrap'],'gates':result['gates']}),flush=True)


if __name__ == '__main__':main()
