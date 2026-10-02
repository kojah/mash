"""Fixed seed discovery on complete common-ready raw/masked local directions."""
import argparse
from collections import Counter
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
from qmsum_semantic_vectors import PREFLIGHT
from sparse_seed_topics import discover,labels,pair_counts


LABELS='064ff3731955b515a07e12e3897102e2cd9d137ac91c43ec222e393a2e492d08'


def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()


def nonoverlap(assignments,gold,ordinals):
    if len(ordinals)!=len(assignments) or any(b<=a for a,b in zip(ordinals,ordinals[1:])):raise ValueError('original source ordinal order required')
    result=pair_counts(assignments,gold)
    for i,g in enumerate(gold):
        if g is None:continue
        for j in range(i+1,len(gold)):
            if ordinals[j]-ordinals[i]>4:break
            if gold[j] is None:continue
            positive=gold[j]==g;predicted=assignments[j]==assignments[i]
            key='tp' if positive and predicted else 'fp' if predicted else 'fn' if positive else 'tn'
            result[key]-=1;result['pairs']-=1
    return result


def experiment(a):
    start=time.perf_counter();cpu=time.process_time();vector_path=a.vectors/'results.json';vectors=json.loads(vector_path.read_text())
    if vectors['preflight_sha256']!=PREFLIGHT or digest(a.labels/'results.json')!=LABELS:raise ValueError('fixed vector support/label capture required')
    capture,source,meetings=load(a.source)
    if (capture,source)!=(vectors['capture_sha256'],vectors['source_sha256']):raise ValueError('source drift')
    for name,pin in vectors['code_pins'].items():
        if digest(Path(__file__).with_name(name))!=pin:raise ValueError('frozen representation implementation drift')
    dense={}
    for name in ['raw','masked']:
        filename=f'{name}-vectors.npy';d=vectors['files'][filename]
        if digest(a.vectors/filename)!=d['sha256']:raise ValueError('frozen vectors drift')
        dense[name]=np.load(a.vectors/filename,mmap_mode='r',allow_pickle=False)
        if dense[name].shape!=(88336,384) or not np.isfinite(dense[name]).all():raise ValueError('incomplete finite vector matrix')
    label_capture=json.loads((a.labels/'results.json').read_text());d=label_capture['matrices']['exact_markers']
    for filename,pin in d['files'].items():
        if digest(a.labels/filename)!=pin:raise ValueError('source label counts drift')
    arrays=[np.load(a.labels/f'exact_markers-{k}.npy',allow_pickle=False) for k in ['data','indices','indptr']]
    counts=sparse.csr_matrix(tuple(arrays),shape=tuple(d['shape']))
    baseline=json.loads((a.baseline/'results.json').read_text())
    if digest(a.baseline/'results.json')!=label_capture['baseline_sha256']:raise ValueError('frozen vocabulary reference drift')
    if digest(a.baseline/'idf.npy')!=baseline['idf_sha256'] or digest(a.baseline/'vocabulary.json')!=baseline['vocabulary_sha256']:raise ValueError('source IDF drift')
    idf=np.load(a.baseline/'idf.npy');terms=json.loads((a.baseline/'vocabulary.json').read_text())
    common=np.array([all(v['status']=='ready' for v in row['variants'].values()) for row in vectors['rows']],dtype=bool)
    if len(common)!=88336 or int(common.sum())!=84434:raise ValueError('complete common readiness drift')
    setup_ms=(time.perf_counter()-start)*1000;offset=0;records=[];timings=[];totals={};far_totals={};sizes={}
    for name in ['raw','masked']:
        for reverse in [False,True]:
            key=name+('_reverse' if reverse else '_forward');totals[key]=Counter();far_totals[key]=Counter();sizes[key]=[]
    for meeting in meetings:
        size=len(meeting.turns);selected=np.flatnonzero(common[offset:offset+size]);members,_=membership(meeting)
        gold=[members[i][0] if len(members[i])==1 else None for i in selected]
        for name in ['raw','masked']:
            matrix=sparse.csr_matrix(dense[name][offset+selected])
            for reverse in [False,True]:
                key=name+('_reverse' if reverse else '_forward');begin=time.perf_counter();discovery=discover(matrix,reverse=reverse)
                discovery_ms=(time.perf_counter()-begin)*1000;latencies=discovery.pop('latencies_ms');assigned=discovery['assignments']
                begin=time.perf_counter();descriptors=labels(counts[offset+selected],assigned,idf,terms);label_ms=(time.perf_counter()-begin)*1000
                all_pairs=pair_counts(assigned,gold);far_pairs=nonoverlap(assigned,gold,selected.tolist())
                totals[key].update(all_pairs);far_totals[key].update(far_pairs);group_sizes=Counter(assigned);sizes[key].extend(group_sizes.values())
                records.append({'meeting':meeting.ordinal,'policy':key,'source_ordinals':selected.tolist(),
                                'excluded_source_ordinals':np.flatnonzero(~common[offset:offset+size]).tolist(),
                                'discovery':discovery,'labels':descriptors,'group_sizes':{str(k):v for k,v in group_sizes.items()},
                                'all_pair_counts':all_pairs,'nonoverlap_pair_counts':far_pairs})
                timings.append({'meeting':meeting.ordinal,'policy':key,'discovery_ms':discovery_ms,'label_ms':label_ms,'insert_latencies_ms':latencies})
        offset+=size
        if meeting.ordinal%10==0:print(json.dumps({'processed_meetings':meeting.ordinal+1}),flush=True)
    if offset!=88336 or len(records)!=162*4:raise ValueError('incomplete source discovery coverage')
    summary={key:{'all_pair_metrics':metrics(totals[key]),'nonoverlap_pair_metrics':metrics(far_totals[key]),
                  'groups':len(sizes[key]),'singleton_groups':sum(n==1 for n in sizes[key]),'largest_group':max(sizes[key]),
                  'median_group_size':float(np.median(sizes[key]))} for key in totals}
    return {'schema_version':1,'scope':'complete_common_support_semantic_seed_control_not_production_topics',
            'source_sha256':source,'capture_sha256':capture,'vectors_sha256':digest(vector_path),'label_counts_sha256':LABELS,
            'source_turns':offset,'common_ready_rows':int(common.sum()),'status_counts':vectors['status_counts'],
            'protocol':{'threshold':.55,'seeds':'immutable_original_source_vectors','seed_tie':'first_created','equality':'join',
                        'maximum_seeds_per_meeting':2048,'arrival':['forward','reverse'],'reset':'per_meeting',
                        'representations':['raw','masked'],'gold_support':'singleton_known','nonoverlap_exclusion_radius':4,
                        'nonoverlap_prespecified':True,'labels':'exact_marker_cleaned_original_counts_frozen_source_idf_top8',
                        'new_inference_calls':0,'held_out_sources_read':False,'semantic_ancestors':False,'lifecycle':False,'multi_tags':False},
            'summary':summary,'records':records,'code_pins':{name:digest(Path(__file__).with_name(name)) for name in
                         ['qmsum_semantic_discovery.py','sparse_seed_topics.py','qmsum.py','qmsum_seed_discovery.py','uv.lock']},
            'timings':{'setup_ms':setup_ms,'runs':timings,'after_imports_ms':(time.perf_counter()-start)*1000,
                       'cpu_ms':(time.process_time()-cpu)*1000,'peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}}


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for key in ['source','vectors','labels','baseline','output-directory']:p.add_argument('--'+key,type=Path,required=True)
    p.add_argument('--compare',type=Path);a=p.parse_args()
    if a.output_directory.exists() or a.output_directory.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    a.output_directory.mkdir(parents=True);r=experiment(a)
    if a.compare:compare(json.loads(a.compare.read_text()),json.loads(json.dumps(r,allow_nan=False)))
    with (a.output_directory/'results.json').open('x') as f:json.dump(r,f,indent=2,allow_nan=False)
    print(json.dumps({'summary':r['summary'],'timings':{k:v for k,v in r['timings'].items() if k!='runs'}}),flush=True)


if __name__=='__main__':main()
