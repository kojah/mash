"""Prospective bounded-block geometry control, separate from topic discovery."""
import argparse
from collections import Counter,defaultdict
from dataclasses import asdict
from enum import StrEnum
import hashlib
import json
import math
from pathlib import Path
import resource
import time

import numpy as np
from threadpoolctl import threadpool_limits

from discovery_inputs import read,digest,UNITS
from episode_blocks import pool,block_counts,lexical
from qmsum_hierarchy import components
from qmsum_semantic_units import VECTORS
from qmsum_semantic_discovery import LABELS
from return_ranking import evaluate,fusion
from semantic_units import aggregate


class Method(StrEnum):
    RAW_DENSE='raw_dense'
    CAUSAL_DENSE='causal_five_dense'
    BLOCK_DENSE='block_dense'
    RAW_LEXICAL='raw_lexical'
    BLOCK_LEXICAL='block_lexical'
    RAW_RRF='raw_rrf60'
    BLOCK_RRF='block_rrf60'
    TEMPORAL='chronological_distance'


def summarize(records):
    result={}
    for split in ['all','development','reserved_internal']:
        selected=[r for r in records if split=='all' or r['split']==split];methods={}
        for method in Method:
            rows=[(r['meeting'],q['topic'],q['metrics'][method]) for r in selected for q in r['queries']]
            topics=defaultdict(list)
            for meeting,topic,metric in rows:topics[meeting,topic].append(metric)
            mean=lambda values:math.fsum(values)/len(values) if values else None
            methods[method]={'queries':len(rows),'topics':len(topics),'map':mean([m['ap'] for _,_,m in rows]),
                'mrr':mean([m['rr'] for _,_,m in rows]),'hit10':mean([int(m['hit10']) for _,_,m in rows]),
                'topic_macro_map':mean([mean([m['ap'] for m in values]) for values in topics.values()]),
                'zero_return_queries':sum(m['returned']==0 for _,_,m in rows)}
        result[split]={'meetings':len(selected),'return_supported_meetings':sum(bool(r['queries']) for r in selected),'methods':methods}
    return result


def experiment(a):
    start=time.perf_counter();cpu=time.process_time();data=read(a);setup_ms=(time.perf_counter()-start)*1000
    records=[];timings=[];offset=0;direction_offset=0;all_directions=[];totals=Counter();limits=Counter()
    with threadpool_limits(limits=1):
        for meeting in data.meetings:
            size=len(meeting.turns);raw=data.raw[offset:offset+size];ready=data.ready[offset:offset+size];tokens=data.tokens[offset:offset+size]
            begin=time.perf_counter();cpu_begin=time.process_time();blocks,owner,directions=pool(raw,ready,tokens)
            pool_ms=(time.perf_counter()-begin)*1000;pool_cpu_ms=(time.process_time()-cpu_begin)*1000
            context,units=aggregate(raw,ready,tokens)
            if data.lineage[meeting.ordinal]!={'meeting':meeting.ordinal,'source_offset':offset,'units':[asdict(u) for u in units]}:
                raise ValueError('original causal lineage drift')
            usable=np.array([ready[i] and not blocks[owner[i]].cancelled for i in range(size)],dtype=bool)
            selected=np.flatnonzero(usable);gold,spans=components(meeting,selected.tolist())
            known=np.array([g is not None for g in gold]);positions=selected[known]
            labels=np.array([g for g in gold if g is not None],dtype=np.int64);parts=np.array([c for g,c in zip(gold,spans) if g is not None],dtype=np.int64)
            block_ids=owner[positions];distance=np.abs(positions[:,None]-positions[None,:])
            different=(labels[:,None]!=labels[None,:])|(parts[:,None]!=parts[None,:])
            eligible=(distance>4)&(block_ids[:,None]!=block_ids[None,:])&different
            relevant=eligible&(labels[:,None]==labels[None,:]);queries=np.flatnonzero(relevant.any(axis=1))
            begin=time.perf_counter();counts=data.counts[offset:offset+size];combined=block_counts(counts,owner,len(blocks))
            raw_lex=lexical(counts[positions],data.idf);pooled_lex=lexical(combined,data.idf)[block_ids]
            raw_dense=np.asarray(raw[positions]);causal_dense=context[positions];block_dense=directions[block_ids]
            scores={Method.RAW_DENSE:raw_dense[queries]@raw_dense.T,
                    Method.CAUSAL_DENSE:causal_dense[queries]@causal_dense.T,
                    Method.BLOCK_DENSE:block_dense[queries]@block_dense.T,
                    Method.RAW_LEXICAL:(raw_lex[queries]@raw_lex.T).toarray(),
                    Method.BLOCK_LEXICAL:(pooled_lex[queries]@pooled_lex.T).toarray()}
            for matrix in scores.values():np.clip(matrix,-1,1,out=matrix)
            scoring_ms=(time.perf_counter()-begin)*1000;query_records=[];begin=time.perf_counter()
            for k,q in enumerate(queries):
                candidates=np.flatnonzero(eligible[q]);ordinals=positions[candidates];truth=relevant[q,candidates]
                values={method:matrix[k,candidates] for method,matrix in scores.items()}
                values[Method.RAW_RRF]=fusion(values[Method.RAW_DENSE],values[Method.RAW_LEXICAL],ordinals)
                values[Method.BLOCK_RRF]=fusion(values[Method.BLOCK_DENSE],values[Method.BLOCK_LEXICAL],ordinals)
                values[Method.TEMPORAL]=1/(1+np.abs(ordinals-positions[q]))
                query_records.append({'source_ordinal':int(positions[q]),'topic':int(labels[q]),'component':int(parts[q]),
                    'candidates':len(candidates),'metrics':{method:evaluate(values[method],ordinals,truth) for method in Method}})
            ranking_ms=(time.perf_counter()-begin)*1000
            mixture=Counter();known_rows=0
            membership_by_source={int(i):g for i,g in zip(selected,gold)}
            for b in blocks:
                assigned=[membership_by_source.get(i) for i in range(b.start,b.end+1)]
                topics=Counter(g for g in assigned if g is not None);known_rows+=sum(topics.values())
                mixture['zero_known_blocks' if not topics else 'single_known_topic_blocks' if len(topics)==1 else 'mixed_known_topic_blocks']+=1
            local={'sources':size,'ready':int(ready.sum()),'usable':len(selected),'known':len(positions),'blocks':len(blocks),
                   'cancelled_blocks':sum(b.cancelled for b in blocks),'eligible_pairs':int(eligible.sum())//2,
                   'return_pairs':int(relevant.sum())//2,'return_queries':len(queries),'no_return_known_queries':len(positions)-len(queries),**dict(mixture)}
            if known_rows!=len(positions):raise ValueError('block singleton source accounting drift')
            totals.update(local);limits.update(b.close for b in blocks);all_directions.append(directions)
            records.append({'meeting':meeting.ordinal,'source_offset':offset,'split':'reserved_internal' if meeting.ordinal%3==0 else 'development',
                'blocks':[asdict(b) for b in blocks],'block_directions_offset':direction_offset,
                'block_directions_sha256':hashlib.sha256(np.asarray(directions,dtype='<f8').tobytes()).hexdigest(),
                'known_source_ordinals':positions.tolist(),'excluded_source_ordinals':np.flatnonzero(~usable).tolist(),
                'support':local,'queries':query_records})
            timings.append({'meeting':meeting.ordinal,'pool_ms':pool_ms,'pool_cpu_ms':pool_cpu_ms,'scoring_ms':scoring_ms,'ranking_ms':ranking_ms})
            offset+=size;direction_offset+=len(blocks)
            if meeting.ordinal%10==0:print(json.dumps({'processed_meetings':meeting.ordinal+1,'return_queries':totals['return_queries']}),flush=True)
    if offset!=88336 or len(records)!=162 or totals['ready']!=84434:raise ValueError('complete original source required')
    path=a.output_directory/'block-directions.npy';matrix=np.vstack(all_directions);np.save(path,matrix,allow_pickle=False)
    return {'schema_version':1,'scope':'complete_training_bounded_block_conditional_geometry_not_selection',
        'source_sha256':data.source,'source_capture_sha256':data.capture,'vectors_sha256':VECTORS,'units_sha256':UNITS,'counts_sha256':LABELS,
        'protocol':{'block_turn_limit':16,'original_model_token_sum_limit':512,'end_priority':'turn_then_token','gaps':'flush_without_compaction',
            'dense':'equal_raw_float64_unit_direction_sum_then_l2_at_block_close','cancellation_norm':1e-12,
            'snapshot':'final_closed_block_representations_not_online_draft_quality','lexical':'original_count_sum_sublinear_tf_times_frozen_full_training_source_idf_then_l2',
            'pooling_includes':'unknown_multi_and_original_common_ready_sources','matched_cancellation_exclusions':True,
            'eligibility':'singleton_known_gap>4_distinct_blocks_and_not_same_native_topic_component','positive':'same_native_topic_different_component',
            'ranking':'positive_scores_only_round12_descending_then_original_ordinal','rrf_k':60,'hit_k':10,
            'partition':'ordinal_mod3_zero_reserved_internal54_other_development108','partition_is_pristine_heldout':False,
            'tuning':False,'additional_inference':0,'llm_calls':0,'held_out_sources_read':False,'full_ingestion_gate':False},
        'support':dict(totals),'block_closes':dict(limits),'summary':summarize(records),'records':records,
        'files':{path.name:{'sha256':digest(path),'shape':list(matrix.shape),'dtype':str(matrix.dtype)}},
        'code_pins':{name:digest(Path(__file__).with_name(name)) for name in ['qmsum_block_geometry.py','episode_blocks.py','return_ranking.py',
            'discovery_inputs.py','semantic_units.py','qmsum_hierarchy.py','qmsum.py','uv.lock']},
        'timings':{'setup_ms':setup_ms,'after_imports_ms':(time.perf_counter()-start)*1000,'cpu_ms':(time.process_time()-cpu)*1000,
                   'peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,'meetings':timings}}


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for key in ['source','vectors','units','labels','baseline','reference-manifest','output-directory']:p.add_argument('--'+key,type=Path,required=True)
    p.add_argument('--compare',type=Path);a=p.parse_args()
    if a.output_directory.exists() or a.output_directory.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    a.output_directory.mkdir(parents=True);result=experiment(a)
    if a.compare:
        old=json.loads(a.compare.read_text())
        if json.dumps({k:v for k,v in old.items() if k!='timings'},sort_keys=True,allow_nan=False)!=json.dumps({k:v for k,v in result.items() if k!='timings'},sort_keys=True,allow_nan=False):
            raise ValueError('complete geometry repeat differs')
    with (a.output_directory/'results.json').open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps({'support':result['support'],'summary':result['summary'],'timings':{k:v for k,v in result['timings'].items() if k!='meetings'}}),flush=True)


if __name__=='__main__':main()
