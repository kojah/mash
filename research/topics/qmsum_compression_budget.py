"""Source-only necessary centroid counts; no clustering or semantic selection."""
import argparse
from collections import Counter
import json
from pathlib import Path
import resource
import time

import numpy as np
from threadpoolctl import threadpool_limits

from compression_budget import Frame,graph,bound
from discovery_inputs import read,digest
from episode_blocks import block_counts,lexical
from qmsum_unique_blocks import GEOMETRY


def experiment(a):
    start=time.perf_counter();cpu=time.process_time()
    if digest(a.geometry/'results.json')!=GEOMETRY:raise ValueError('immutable complete block capture required')
    original=json.loads((a.geometry/'results.json').read_text());data=read(a)
    if original['source_sha256']!=data.source or original['support']['cancelled_blocks']!=0 or len(original['records'])!=162:
        raise ValueError('matched no-cancellation complete source required')
    for name,pin in original['code_pins'].items():
        if digest(Path(__file__).with_name(name))!=pin:raise ValueError('immutable block implementation drift')
    pin=original['files']['block-directions.npy']['sha256']
    if digest(a.geometry/'block-directions.npy')!=pin:raise ValueError('block directions drift')
    directions=np.load(a.geometry/'block-directions.npy',mmap_mode='r',allow_pickle=False)
    setup=(time.perf_counter()-start)*1000;records=[];timings=[];support=Counter()
    with threadpool_limits(limits=1):
        for meeting,row in zip(data.meetings,original['records']):
            if meeting.ordinal!=row['meeting']:raise ValueError('complete original source order required')
            size=len(meeting.turns);offset=row['source_offset'];blocks=row['blocks'];owner=np.full(size,-1,dtype=np.int64)
            for b in blocks:owner[b['start']:b['end']+1]=b['ordinal']
            if not np.array_equal(owner>=0,data.ready[offset:offset+size]):raise ValueError('ready lineage drift')
            begin=time.perf_counter();lex=lexical(block_counts(data.counts[offset:offset+size],owner,len(blocks)),data.idf)
            lo=row['block_directions_offset'];dense=directions[lo:lo+len(blocks)]
            lexical_cosine=(lex@lex.T).toarray();dense_cosine=dense@dense.T
            features_ms=(time.perf_counter()-begin)*1000;begin=time.perf_counter()
            views={Frame.LEXICAL:graph(lexical_cosine),Frame.DENSE:graph(dense_cosine),
                Frame.HYBRID:graph((lexical_cosine+dense_cosine)/2)}
            records.append({'meeting':meeting.ordinal,'split':row['split'],'views':views})
            timings.append({'meeting':meeting.ordinal,'features_and_scores_ms':features_ms,
                'feasibility_validation_ms':(time.perf_counter()-begin)*1000})
            support.update(sources=size,ready=int((owner>=0).sum()),blocks=len(blocks))
            if meeting.ordinal%20==0:print(json.dumps({'processed_meetings':meeting.ordinal+1}),flush=True)
    if support!={'sources':88336,'ready':84434,'blocks':8196}:raise ValueError('complete support drift')
    summaries={}
    for split in ['all','development','reserved_internal']:
        selected=[r for r in records if split=='all' or r['split']==split]
        summaries[split]={frame:bound([r['views'][frame] for r in selected]) for frame in Frame}
    return {'schema_version':1,'scope':'full_training_source_only_per_meeting_centroid_fidelity_necessary_bound_not_selection',
        'source_sha256':data.source,'source_capture_sha256':data.capture,'geometry_sha256':GEOMETRY,'block_file_sha256':pin,
        'support':dict(support),'protocol':{'targets':'COMPRESSION_BUDGET.md_frozen_before_outcomes',
            'unit_centroid_minimum_cosine':.9,'minimum_passing_fraction':'19/20','maximum_group_fraction':'1/2',
            'necessary_pair_cosine':2*.9*.9-1,'conservative_numerical_margin':1e-12,
            'lower_bound':'max_nonempty_meetings_sum_isolated_minus_floor_totalblocks_over20',
            'views':['original_fulltraining_sparse_lexical','unchanged_pooled_dense','equal_energy_direct_sum'],
            'ownership_scope':'separate_meeting_groups_not_global_cross_thread','partition':'explored_internal54_reserved108_development_not_pristine',
            'clusters_fit':0,'gold_used_to_select_thresholds':False,'new_inference':0,'llm_calls':0,'held_out_read':False,
            'topic_label_quality_measured':False,'ingestion_gate':False,'positive_verdict':'not_ruled_out_never_proven_feasible'},
        'summary':summaries,'records':records,'code_pins':{n:digest(Path(__file__).with_name(n)) for n in
            ['qmsum_compression_budget.py','compression_budget.py','COMPRESSION_BUDGET.md','episode_blocks.py','discovery_inputs.py','qmsum.py','uv.lock']},
        'timings':{'setup_ms':setup,'after_imports_ms':(time.perf_counter()-start)*1000,'cpu_ms':(time.process_time()-cpu)*1000,
            'peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,'stages':timings}}


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['source','vectors','units','labels','baseline','reference-manifest','geometry','output']:p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--compare',type=Path);a=p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    result=experiment(a)
    if a.compare:
        old=json.loads(a.compare.read_text())
        if json.dumps({k:v for k,v in old.items() if k!='timings'},sort_keys=True,allow_nan=False)!=json.dumps({k:v for k,v in result.items() if k!='timings'},sort_keys=True,allow_nan=False):raise ValueError('complete feasibility repeat differs')
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps({'support':result['support'],'summary':result['summary'],'timings':{k:v for k,v in result['timings'].items() if k!='stages'}}),flush=True)


if __name__=='__main__':main()
