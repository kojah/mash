"""Complete matched-source semantic unit comparison at fixed discovery policy."""
import argparse
from collections import Counter
from dataclasses import asdict
import hashlib
import json
from pathlib import Path
import resource
import time

import numpy as np
from scipy import sparse

from external_context import compare
from qmsum import load,membership
from qmsum_seed_discovery import metrics
from qmsum_semantic_discovery import LABELS,nonoverlap
from semantic_units import aggregate,Status
from sparse_seed_topics import discover,labels,pair_counts


VECTORS='05642c1a77dab33d2940c39979b0ac4e4b420420debcb30ca529d7bad99bdf98'
DISCOVERY='86efcb57e3f9a546976e660e155513c64c35ab89254796d92e11245c9fdf116c'


def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()


def experiment(a):
    start=time.perf_counter();cpu=time.process_time()
    if digest(a.vectors/'results.json')!=VECTORS or digest(a.discovery/'results.json')!=DISCOVERY or digest(a.labels/'results.json')!=LABELS:
        raise ValueError('original audited representation/discovery/count captures required')
    vectors=json.loads((a.vectors/'results.json').read_text());old=json.loads((a.discovery/'results.json').read_text())
    capture,source,meetings=load(a.source)
    if (capture,source)!=(vectors['capture_sha256'],vectors['source_sha256']) or old['vectors_sha256']!=VECTORS:raise ValueError('source drift')
    for recorded in [vectors,old]:
        for name,pin in recorded['code_pins'].items():
            if digest(Path(__file__).with_name(name))!=pin:raise ValueError('frozen implementation drift')
    path=a.vectors/'raw-vectors.npy'
    if digest(path)!=vectors['files'][path.name]['sha256']:raise ValueError('raw vector drift')
    raw=np.load(path,mmap_mode='r',allow_pickle=False)
    if raw.shape!=(88336,384) or not np.isfinite(raw).all():raise ValueError('complete finite vectors required')
    lc=json.loads((a.labels/'results.json').read_text());d=lc['matrices']['exact_markers']
    for filename,pin in d['files'].items():
        if digest(a.labels/filename)!=pin:raise ValueError('source label count drift')
    arrays=[np.load(a.labels/f'exact_markers-{k}.npy',allow_pickle=False) for k in ['data','indices','indptr']]
    counts=sparse.csr_matrix(tuple(arrays),shape=tuple(d['shape']))
    baseline=json.loads((a.baseline/'results.json').read_text())
    if digest(a.baseline/'results.json')!=lc['baseline_sha256'] or digest(a.baseline/'idf.npy')!=baseline['idf_sha256'] or digest(a.baseline/'vocabulary.json')!=baseline['vocabulary_sha256']:
        raise ValueError('original source vocabulary/IDF drift')
    idf=np.load(a.baseline/'idf.npy');terms=json.loads((a.baseline/'vocabulary.json').read_text())
    ready=np.array([all(v['status']=='ready' for v in row['variants'].values()) for row in vectors['rows']],dtype=bool)
    tokens=np.array([row['variants']['raw']['tokens'] for row in vectors['rows']],dtype=np.int64)
    if len(ready)!=88336 or int(ready.sum())!=84434:raise ValueError('complete support required')
    original={(r['meeting'],r['policy']):r for r in old['records']};records=[];timings=[];lineage=[];offset=0
    totals={k:Counter() for k in ['raw_forward','raw_reverse','context_forward','context_reverse']}
    far_totals={k:Counter() for k in totals};sizes={k:[] for k in totals};status_counts=Counter();limits=Counter();selected_rows=0
    setup_ms=(time.perf_counter()-start)*1000
    for meeting in meetings:
        size=len(meeting.turns);begin=time.perf_counter()
        context,units=aggregate(raw[offset:offset+size],ready[offset:offset+size],tokens[offset:offset+size])
        aggregate_ms=(time.perf_counter()-begin)*1000;status_counts.update(u.status.value for u in units);limits.update(u.limit.value for u in units)
        selected=np.array([u.ordinal for u in units if u.status==Status.READY],dtype=np.int64);selected_rows+=len(selected)
        lineage.append({'meeting':meeting.ordinal,'source_offset':offset,'units':[asdict(u) for u in units]})
        members,_=membership(meeting);gold=[members[i][0] if len(members[i])==1 else None for i in selected]
        unchanged=np.array_equal(selected,np.flatnonzero(ready[offset:offset+size]))
        for name,dense in [('raw',raw[offset:offset+size]),('context',context)]:
            matrix=sparse.csr_matrix(dense[selected])
            for reverse in [False,True]:
                key=name+('_reverse' if reverse else '_forward');begin=time.perf_counter();discovery=discover(matrix,reverse=reverse)
                discovery_ms=(time.perf_counter()-begin)*1000;latencies=discovery.pop('latencies_ms');assigned=discovery['assignments']
                begin=time.perf_counter();descriptors=labels(counts[offset+selected],assigned,idf,terms);label_ms=(time.perf_counter()-begin)*1000
                all_pairs=pair_counts(assigned,gold);far_pairs=nonoverlap(assigned,gold,selected.tolist());group_sizes=Counter(assigned)
                entry={'meeting':meeting.ordinal,'policy':key,'source_ordinals':selected.tolist(),
                       'excluded_source_ordinals':sorted(set(range(size))-set(selected.tolist())),
                       'discovery':discovery,'labels':descriptors,'group_sizes':{str(k):v for k,v in group_sizes.items()},
                       'all_pair_counts':all_pairs,'nonoverlap_pair_counts':far_pairs}
                if name=='raw' and unchanged and entry!=original[(meeting.ordinal,key)]:raise ValueError('exact original baseline replay diverged')
                records.append(entry);totals[key].update(all_pairs);far_totals[key].update(far_pairs);sizes[key].extend(group_sizes.values())
                timings.append({'meeting':meeting.ordinal,'policy':key,'aggregate_ms':aggregate_ms if name=='context' else 0,
                                'discovery_ms':discovery_ms,'label_ms':label_ms,'insert_latencies_ms':latencies})
        offset+=size
        if meeting.ordinal%10==0:print(json.dumps({'processed_meetings':meeting.ordinal+1}),flush=True)
    if offset!=88336 or len(records)!=648 or len(lineage)!=162:raise ValueError('incomplete source evaluation')
    summary={k:{'all_pair_metrics':metrics(totals[k]),'nonoverlap_pair_metrics':metrics(far_totals[k]),'groups':len(sizes[k]),
                'singleton_groups':sum(n==1 for n in sizes[k]),'largest_group':max(sizes[k]),'median_group_size':float(np.median(sizes[k]))} for k in totals}
    return {'schema_version':1,'scope':'complete_causal_semantic_unit_control_not_topic_method_selection',
            'source_sha256':source,'capture_sha256':capture,'vectors_sha256':VECTORS,'discovery_sha256':DISCOVERY,'label_counts_sha256':LABELS,
            'source_turns':offset,'original_ready_rows':int(ready.sum()),'common_ready_rows':selected_rows,'status_counts':dict(status_counts),'limits':dict(limits),
            'protocol':{'context':'float64_sum_of_current_and_up_to_four_contiguous_prior_common_ready_raw_unit_vectors_then_l2',
                        'maximum_sum_original_model_tokens':512,'unsupported_prior':'stop_without_compacting','cancellation_norm_limit':1e-12,
                        'arrival':['forward','reverse'],'threshold':.55,'seed_tie':'first_created','equality':'join','reset':'per_meeting',
                        'source_label_counts':'exact_markers_original_source_idf_top8','nonoverlap_radius':4,'nonoverlap_prespecified':True,
                        'new_inference_calls':0,'held_out_sources_read':False,'semantic_ancestors':False,'lifecycle':False,'multi_tags':False},
            'summary':summary,'records':records,'lineage':lineage,'code_pins':{name:digest(Path(__file__).with_name(name)) for name in
              ['qmsum_semantic_units.py','semantic_units.py','qmsum_semantic_discovery.py','sparse_seed_topics.py','qmsum.py','qmsum_seed_discovery.py','uv.lock']},
            'timings':{'setup_ms':setup_ms,'runs':timings,'after_imports_ms':(time.perf_counter()-start)*1000,
                       'cpu_ms':(time.process_time()-cpu)*1000,'peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}}


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for key in ['source','vectors','discovery','labels','baseline','output-directory']:p.add_argument('--'+key,type=Path,required=True)
    p.add_argument('--compare',type=Path);a=p.parse_args()
    if a.output_directory.exists() or a.output_directory.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    a.output_directory.mkdir(parents=True);r=experiment(a)
    if a.compare:compare(json.loads(a.compare.read_text()),json.loads(json.dumps(r,allow_nan=False)))
    with (a.output_directory/'results.json').open('x') as f:json.dump(r,f,indent=2,allow_nan=False)
    print(json.dumps({'summary':r['summary'],'support':r['status_counts'],'limits':r['limits'],
                      'timings':{k:v for k,v in r['timings'].items() if k!='runs'}}),flush=True)


if __name__=='__main__':main()
