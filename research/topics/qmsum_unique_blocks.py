"""Frozen unique-block return geometry, with duplicate-free branch fusion."""
import argparse
from collections import Counter,defaultdict
from dataclasses import asdict
import json
import math
from pathlib import Path
import resource
import time

import numpy as np
from threadpoolctl import threadpool_limits

from block_ranking import group,maximum,metrics
from discovery_inputs import read,digest
from episode_blocks import block_counts,lexical
from qmsum_block_geometry import Method
from qmsum_hierarchy import components
from return_ranking import fusion
from semantic_units import aggregate


GEOMETRY='3f0b2c8632b8dda5ad34f3d058032cb24afb99d776998060e603b92cac63fdb1'


def summarize(records):
    result={}
    for split in ['all','development','reserved_internal']:
        selected=[r for r in records if split=='all' or r['split']==split];methods={}
        for method in Method:
            rows=[(r['meeting'],q['topic'],q['metrics'][method]) for r in selected for q in r['queries']];topics=defaultdict(list)
            for meeting,topic,metric in rows:topics[meeting,topic].append(metric)
            mean=lambda values:math.fsum(values)/len(values) if values else None
            methods[method]={'queries':len(rows),'topics':len(topics),'map':mean([m['ap'] for _,_,m in rows]),
                'mrr':mean([m['rr'] for _,_,m in rows]),'hit10':mean([int(m['hit10']) for _,_,m in rows]),
                'ndcg10':mean([m['ndcg10'] for _,_,m in rows]),
                'topic_macro_map':mean([mean([m['ap'] for m in values]) for values in topics.values()]),
                'topic_macro_ndcg10':mean([mean([m['ndcg10'] for m in values]) for values in topics.values()]),
                'zero_return_queries':sum(m['returned']==0 for _,_,m in rows)}
        result[split]={'meetings':len(selected),'return_supported_meetings':sum(bool(r['queries']) for r in selected),'methods':methods}
    return result


def experiment(a):
    start=time.perf_counter();cpu=time.process_time()
    if digest(a.geometry/'results.json')!=GEOMETRY:raise ValueError('audited immutable complete block geometry required')
    old=json.loads((a.geometry/'results.json').read_text());data=read(a)
    if (old['source_sha256']!=data.source or old['support']['cancelled_blocks']!=0 or old['support']['usable']!=84434
            or len(old['records'])!=162):raise ValueError('matched no-cancellation source control required')
    for name,pin in old['code_pins'].items():
        if digest(Path(__file__).with_name(name))!=pin:raise ValueError('frozen geometry implementation drift')
    if digest(a.geometry/'block-directions.npy')!=old['files']['block-directions.npy']['sha256']:raise ValueError('block direction file drift')
    directions=np.load(a.geometry/'block-directions.npy',mmap_mode='r',allow_pickle=False)
    records=[];timings=[];totals=Counter();setup_ms=(time.perf_counter()-start)*1000
    with threadpool_limits(limits=1):
        for meeting,original in zip(data.meetings,old['records']):
            if meeting.ordinal!=original['meeting']:raise ValueError('complete original source order required')
            size=len(meeting.turns);offset=original['source_offset'];owner=np.full(size,-1,dtype=np.int64)
            for b in original['blocks']:owner[b['start']:b['end']+1]=b['ordinal']
            if not np.array_equal(owner>=0,data.ready[offset:offset+size]):raise ValueError('original ready lineage drift')
            lo=original['block_directions_offset'];block_vectors=directions[lo:lo+len(original['blocks'])]
            raw=data.raw[offset:offset+size];context,units=aggregate(raw,data.ready[offset:offset+size],data.tokens[offset:offset+size])
            if data.lineage[meeting.ordinal]!={'meeting':meeting.ordinal,'source_offset':offset,'units':[asdict(u) for u in units]}:raise ValueError('causal lineage drift')
            positions=np.asarray(original['known_source_ordinals'],dtype=np.int64);gold,parts=components(meeting,positions.tolist())
            if any(g is None for g in gold):raise ValueError('original known support drift')
            gold=np.asarray(gold);parts=np.asarray(parts);distance=np.abs(positions[:,None]-positions[None,:])
            eligible=(distance>4)&(owner[positions,None]!=owner[positions][None,:])&((gold[:,None]!=gold[None,:])|(parts[:,None]!=parts[None,:]))
            relevant=eligible&(gold[:,None]==gold[None,:]);queries=np.flatnonzero(relevant.any(axis=1))
            if positions[queries].tolist()!=[q['source_ordinal'] for q in original['queries']]:raise ValueError('matched query support drift')
            begin=time.perf_counter();counts=data.counts[offset:offset+size];combined=block_counts(counts,owner,len(block_vectors))
            raw_lex=lexical(counts[positions],data.idf);pooled_lex=lexical(combined,data.idf)[owner[positions]]
            raw_dense=np.asarray(raw[positions]);causal_dense=context[positions];block_dense=block_vectors[owner[positions]]
            scores={Method.RAW_DENSE:raw_dense[queries]@raw_dense.T,Method.CAUSAL_DENSE:causal_dense[queries]@causal_dense.T,
                    Method.BLOCK_DENSE:block_dense[queries]@block_dense.T,Method.RAW_LEXICAL:(raw_lex[queries]@raw_lex.T).toarray(),
                    Method.BLOCK_LEXICAL:(pooled_lex[queries]@pooled_lex.T).toarray()}
            for matrix in scores.values():np.clip(matrix,-1,1,out=matrix)
            scoring_ms=(time.perf_counter()-begin)*1000;begin=time.perf_counter();query_records=[]
            for k,q in enumerate(queries):
                candidates=np.flatnonzero(eligible[q]);ordinals=positions[candidates];truth=relevant[q,candidates]
                if len(candidates)!=original['queries'][k]['candidates'] or int(truth.sum())!=original['queries'][k]['metrics'][Method.RAW_DENSE]['relevant']:
                    raise ValueError('original candidate/relevance support drift')
                groups=group(owner[ordinals],ordinals,truth);values={method:maximum(matrix[k,candidates],groups) for method,matrix in scores.items()}
                values[Method.RAW_RRF]=fusion(values[Method.RAW_DENSE],values[Method.RAW_LEXICAL],groups.ordinals)
                values[Method.BLOCK_RRF]=fusion(values[Method.BLOCK_DENSE],values[Method.BLOCK_LEXICAL],groups.ordinals)
                values[Method.TEMPORAL]=maximum(1/(1+np.abs(ordinals-positions[q])),groups)
                support={'candidate_sources':len(candidates),'candidate_blocks':len(groups.blocks),'relevant_sources':int(truth.sum()),
                         'relevant_blocks':int((groups.gains>0).sum()),'mixed_target_blocks':int(((groups.gains>0)&(groups.gains<1)).sum())}
                totals.update(support)
                query_records.append({'source_ordinal':int(positions[q]),'topic':int(gold[q]),'component':int(parts[q]),'support':support,
                    'groups':[{'block':int(b),'earliest_source_ordinal':int(o),'eligible_known_members':len(m),
                               'relevant_known_members':int(truth[m].sum()),'gain':float(g)}
                              for b,o,m,g in zip(groups.blocks,groups.ordinals,groups.members,groups.gains)],
                    'metrics':{method:metrics(values[method],groups) for method in Method}})
            timings.append({'meeting':meeting.ordinal,'scoring_ms':scoring_ms,'grouping_ranking_ms':(time.perf_counter()-begin)*1000})
            records.append({'meeting':meeting.ordinal,'split':original['split'],'queries':query_records})
            if meeting.ordinal%10==0:print(json.dumps({'processed_meetings':meeting.ordinal+1}),flush=True)
    if len(records)!=162 or sum(len(r['queries']) for r in records)!=2410:raise ValueError('complete matched evaluation required')
    return {'schema_version':1,'scope':'complete_training_unique_block_conditional_geometry_not_selection',
        'source_sha256':data.source,'source_capture_sha256':data.capture,'geometry_sha256':GEOMETRY,
        'block_file_sha256':old['files']['block-directions.npy']['sha256'],'original_source_support':old['support'],
        'protocol':{'blocks':'unchanged_original_16turn512token_gap_policy','queries_and_candidate_sources':'exact_original_matched2410',
            'grouping':'physical_block','branch_scores':'maximum_eligible_message_score','identity_ties':'earliest_eligible_source_ordinal',
            'fusion':'deduplicate_branches_before_rrf60','score_ranking':'positive_only_round12_descending_then_identity',
            'binary_relevance':'any_eligible_known_member_same_native_topic_different_span',
            'graded_gain':'relevant_eligible_known_members_divided_by_all_eligible_known_members',
            'ndcg':'linear_gain_over_log2_rankplus1_normalized_by_ideal_top10','unknown_multi':'outside_conditional_gold_not_automatic_noise',
            'partition':'unchanged_explored_internal54_reserved108_development_not_pristine_heldout',
            'snapshot':'final_closed_blocks_not_online_draft','no_direct_message_ap_comparison':True,
            'new_fit':False,'new_inference':0,'llm_calls':0,'held_out_sources_read':False,'frontier_selection':False,'full_ingestion_gate':False},
        'query_summed_support':dict(totals),'summary':summarize(records),'records':records,
        'code_pins':{name:digest(Path(__file__).with_name(name)) for name in ['qmsum_unique_blocks.py','block_ranking.py','qmsum_block_geometry.py',
            'episode_blocks.py','return_ranking.py','discovery_inputs.py','semantic_units.py','qmsum_hierarchy.py','qmsum.py','uv.lock']},
        'timings':{'setup_ms':setup_ms,'after_imports_ms':(time.perf_counter()-start)*1000,'cpu_ms':(time.process_time()-cpu)*1000,
                   'peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,'meetings':timings}}


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for key in ['source','vectors','units','labels','baseline','reference-manifest','geometry','output']:p.add_argument('--'+key,type=Path,required=True)
    p.add_argument('--compare',type=Path);a=p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    result=experiment(a)
    if a.compare:
        old=json.loads(a.compare.read_text())
        if json.dumps({k:v for k,v in old.items() if k!='timings'},sort_keys=True,allow_nan=False)!=json.dumps({k:v for k,v in result.items() if k!='timings'},sort_keys=True,allow_nan=False):
            raise ValueError('complete unique-block repeat differs')
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps({'support':result['query_summed_support'],'summary':result['summary'],'timings':{k:v for k,v in result['timings'].items() if k!='meetings'}}),flush=True)


if __name__=='__main__':main()
