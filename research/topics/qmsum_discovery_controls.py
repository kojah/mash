"""Complete actual discovery comparisons on unchanged audited causal directions."""
import argparse
from collections import Counter
from dataclasses import asdict
import importlib.metadata
import json
from pathlib import Path
import resource
import time

import numpy as np
from sklearn.metrics import adjusted_rand_score

from discovery_controls import discover,Method,BIRCH_SOURCE
from discovery_inputs import read,digest,UNITS
from external_context import compare
from qmsum import membership
from qmsum_seed_discovery import metrics
from qmsum_semantic_discovery import nonoverlap,LABELS
from qmsum_semantic_units import VECTORS
from semantic_units import aggregate,Status
from sparse_seed_topics import labels,pair_counts


def topic_support(assigned,gold,ordinals,topic_count):
    result=[]
    for topic in range(topic_count):
        positions=[i for i,g in enumerate(gold) if g==topic];relevant=0;recovered=0
        for j,i in enumerate(positions):
            for k in positions[j+1:]:
                if ordinals[k]-ordinals[i]>4:
                    relevant+=1;recovered+=assigned[i]==assigned[k]
        result.append({'topic':topic,'rows':len(positions),'far_relevant_pairs':relevant,'far_recovered_pairs':int(recovered),
                       'far_recall':recovered/relevant if relevant else None})
    return result


def experiment(a):
    start=time.perf_counter();cpu=time.process_time();data=read(a);setup_ms=(time.perf_counter()-start)*1000
    offset=0;records=[];timings=[];orders=[];totals={key:Counter() for key in ['forward','reverse']};far={key:Counter() for key in totals}
    sizes={key:[] for key in totals};rare={key:Counter() for key in totals};empty={key:0 for key in totals}
    for meeting in data.meetings:
        size=len(meeting.turns);begin=time.perf_counter();context,units=aggregate(data.raw[offset:offset+size],data.ready[offset:offset+size],data.tokens[offset:offset+size])
        aggregation_ms=(time.perf_counter()-begin)*1000
        if data.lineage[meeting.ordinal]!={'meeting':meeting.ordinal,'source_offset':offset,'units':[asdict(u) for u in units]}:raise ValueError('causal lineage drift')
        selected=np.array([u.ordinal for u in units if u.status==Status.READY],dtype=np.int64)
        if not np.array_equal(selected,np.flatnonzero(data.ready[offset:offset+size])):raise ValueError('matched support drift')
        matrix=context[selected];members,_=membership(meeting);gold=[members[i][0] if len(members[i])==1 else None for i in selected]
        assignments={}
        for reverse in [False,True]:
            order='reverse' if reverse else 'forward';r=discover(matrix,a.method,reverse=reverse);assignments[order]=r.assignments
            begin=time.perf_counter();descriptors=labels(data.counts[offset+selected],r.assignments,data.idf,data.terms);label_ms=(time.perf_counter()-begin)*1000
            all_pairs=pair_counts(r.assignments,gold);far_pairs=nonoverlap(r.assignments,gold,selected.tolist());support=topic_support(r.assignments,gold,selected.tolist(),len(meeting.topics))
            totals[order].update(all_pairs);far[order].update(far_pairs);sizes[order].extend(map(len,r.clusters));empty[order]+=sum(not label for label in descriptors)
            for t in support:
                bucket='zero' if not t['rows'] else 'rare' if t['rows']<=20 else 'common'
                rare[order][bucket+'_topics']+=1;rare[order][bucket+'_rows']+=t['rows'];rare[order][bucket+'_far_relevant']+=t['far_relevant_pairs'];rare[order][bucket+'_far_recovered']+=t['far_recovered_pairs']
                rare[order][bucket+'_zero_far_topics']+=int(t['far_relevant_pairs']==0)
            records.append({'meeting':meeting.ordinal,'order':order,'source_ordinals':selected.tolist(),
                            'excluded_source_ordinals':np.flatnonzero(~data.ready[offset:offset+size]).tolist(),
                            'assignments':r.assignments,'clusters':r.clusters,'state':r.state,'labels':descriptors,'topic_support':support,
                            'all_pair_counts':all_pairs,'nonoverlap_pair_counts':far_pairs})
            timings.append({'meeting':meeting.ordinal,'order':order,'aggregation_ms':aggregation_ms if not reverse else 0,'label_ms':label_ms,**r.timings})
        orders.append({'meeting':meeting.ordinal,'rows':len(selected),'forward_reverse_ari':float(adjusted_rand_score(assignments['forward'],assignments['reverse']))})
        offset+=size
        if meeting.ordinal%10==0:print(json.dumps({'method':a.method,'processed_meetings':meeting.ordinal+1}),flush=True)
    if offset!=88336 or len(records)!=324:raise ValueError('complete source control required')
    summary={order:{'all_pair_metrics':metrics(totals[order]),'nonoverlap_pair_metrics':metrics(far[order]),'groups':len(sizes[order]),
                    'singleton_groups':sum(n==1 for n in sizes[order]),'largest_group':max(sizes[order]),'median_group_size':float(np.median(sizes[order])),
                    'empty_label_groups':empty[order],'rare_topic_counts':dict(rare[order])} for order in totals}
    return {'schema_version':1,'scope':'complete_actual_discovery_at_frozen_uncalibrated_operating_points_not_method_selection',
            'method':a.method,'source_sha256':data.source,'capture_sha256':data.capture,'vectors_sha256':VECTORS,'units_sha256':UNITS,'label_counts_sha256':LABELS,
            'reference':data.reference,'reference_manifest_sha256':digest(a.reference_manifest),'birch_source_sha256':BIRCH_SOURCE,
            'source_turns':offset,'common_ready_rows':int(data.ready.sum()),'seed_control_summary':data.seed_summary,
            'protocol':{'evidence':'unchanged_audited_causal_raw_vector_sums','arrival':['forward','reverse'],'reset':'per_meeting',
                        'birch':{'threshold':.4,'branching_factor':64,'n_clusters':None,'update':'singleton_partial_fit','evaluation':'final_predict_original_order'},
                        'grinch_style':{'mode':'grinch','linkage':'normalized_centroid','cut':.7,'cut_policy':'every_internal_merger_at_least_cutoff',
                                        'neighbors':'exact_leaf','ties':'stable_arrival_and_node_serial','maximum_leaves':2000,'maximum_dimensions':1024},
                        'canonical_partition':'lexicographic_original_source_index_groups','labels':'fixed_exact_marker_original_counts_source_idf_top8',
                        'gold':'singleton_known','far_radius':4,'rare_rows':'1_to20_singleton_known_ready','new_inference_calls':0,
                        'held_out_sources_read':False,'semantic_ancestors':False,'lifecycle':False,'multi_tags':False},
            'versions':{name:importlib.metadata.version(name) for name in ['numpy','scipy','scikit-learn','threadpoolctl']},
            'summary':summary,'order_diagnostics':{'meetings':orders,'mean_ari':float(np.mean([r['forward_reverse_ari'] for r in orders])),
                                               'source_row_weighted_ari':float(np.average([r['forward_reverse_ari'] for r in orders],weights=[r['rows'] for r in orders]))},
            'records':records,'code_pins':{name:digest(Path(__file__).with_name(name)) for name in
              ['qmsum_discovery_controls.py','discovery_controls.py','discovery_inputs.py','semantic_units.py','online_tree.py','data.py',
               'qmsum.py','qmsum_semantic_discovery.py','qmsum_seed_discovery.py','sparse_seed_topics.py','uv.lock']},
            'timings':{'setup_ms':setup_ms,'runs':timings,'after_imports_ms':(time.perf_counter()-start)*1000,
                       'cpu_ms':(time.process_time()-cpu)*1000,'peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}}


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for key in ['source','vectors','units','labels','baseline','reference-manifest','output-directory']:p.add_argument('--'+key,type=Path,required=True)
    p.add_argument('--method',choices=list(Method),type=Method,required=True);p.add_argument('--compare',type=Path);a=p.parse_args()
    if a.output_directory.exists() or a.output_directory.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    a.output_directory.mkdir(parents=True);r=experiment(a)
    if a.compare:compare(json.loads(a.compare.read_text()),json.loads(json.dumps(r,allow_nan=False)))
    with (a.output_directory/'results.json').open('x') as f:json.dump(r,f,indent=2,allow_nan=False)
    print(json.dumps({'method':a.method,'summary':r['summary'],'order':{k:v for k,v in r['order_diagnostics'].items() if k!='meetings'},
                      'timings':{k:v for k,v in r['timings'].items() if k!='runs'}}),flush=True)


if __name__=='__main__':main()
