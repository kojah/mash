"""Complete meeting-local insert-only discovery control on frozen source evidence."""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import re
import resource
import time

import numpy as np
from scipy import sparse

from external_context import compare
from qmsum import load,membership
from sparse_seed_topics import discover,labels,pair_counts


BASELINE='e3c89677f538f65f3e23a5a7de14de6ef9340febb40a408d97ebb275cb11b488'
TOKEN=re.compile(r'(?u)\b\w\w+\b')


def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()


def metrics(counts):
    tp,fp,fn=counts['tp'],counts['fp'],counts['fn']
    return {**dict(counts),'precision':tp/(tp+fp) if tp+fp else None,'recall':tp/(tp+fn) if tp+fn else None,
            'f1':2*tp/(2*tp+fp+fn) if 2*tp+fp+fn else None}


def compare_capture(archived, current):
    # JSON object keys are strings on disk; compare exactly the emitted schema.
    compare(archived,json.loads(json.dumps(current,allow_nan=False)))


def experiment(a):
    start=time.perf_counter();cpu=time.process_time();path=a.baseline/'results.json'
    if digest(path)!=BASELINE:raise ValueError('audited complete source representation required')
    original=json.loads(path.read_text());capture,source,meetings=load(a.source)
    if (capture,source)!=(original['capture_sha256'],original['source_sha256']):raise ValueError('source drift')
    matrices={}
    for name,d in original['matrices'].items():
        for filename,pin in d['files'].items():
            if digest(a.baseline/filename)!=pin:raise ValueError('matrix drift')
        arrays=[np.load(a.baseline/f'{name}-{k}.npy',allow_pickle=False) for k in ['data','indices','indptr']]
        matrices[name]=sparse.csr_matrix(tuple(arrays),shape=tuple(d['shape']))
    for name,pin in original['code_pins'].items():
        if digest(Path(__file__).with_name(name))!=pin:raise ValueError('source implementation drift')
    if digest(a.baseline/'idf.npy')!=original['idf_sha256'] or digest(a.baseline/'vocabulary.json')!=original['vocabulary_sha256']:raise ValueError('label vocabulary drift')
    idf=np.load(a.baseline/'idf.npy');terms=json.loads((a.baseline/'vocabulary.json').read_text())
    common=(matrices['current'].getnnz(axis=1)>0)&(matrices['context'].getnnz(axis=1)>0)
    setup_ms=(time.perf_counter()-start)*1000;offset=0;records=[];timings=[];totals={};sizes={};name_scores={}
    for name in ['current','context']:
        for reverse in [False,True]:
            key=name+('_reverse' if reverse else '_forward');totals[key]=Counter();sizes[key]=[];name_scores[key]=[]
    for meeting in meetings:
        size=len(meeting.turns);selected=np.flatnonzero(common[offset:offset+size]);members,_=membership(meeting)
        gold=[members[i][0] if len(members[i])==1 else None for i in selected]
        names=[set(TOKEN.findall(t.name.lower())) for t in meeting.topics]
        for name in ['current','context']:
            for reverse in [False,True]:
                key=name+('_reverse' if reverse else '_forward');begin=time.perf_counter()
                discovery=discover(matrices[name][offset+selected],reverse=reverse)
                discovery_ms=(time.perf_counter()-begin)*1000;begin=time.perf_counter()
                assignments=discovery['assignments'];descriptors=labels(matrices['counts'][offset+selected],assignments,idf,terms)
                label_ms=(time.perf_counter()-begin)*1000;latencies=discovery.pop('latencies_ms')
                contingency=pair_counts(assignments,gold);totals[key].update(contingency)
                group_sizes=Counter(assignments);sizes[key].extend(group_sizes.values());alignments=[]
                for topic in range(len(names)):
                    support=[j for j,i in enumerate(selected) if topic in members[i]]
                    if not support:continue
                    values=[len(names[topic]&{t['term'] for t in descriptors[assignments[j]]})/len(names[topic]) for j in support]
                    alignments.extend(values)
                name_scores[key].extend(alignments)
                records.append({'meeting':meeting.ordinal,'policy':key,'original_turns':size,'source_ordinals':selected.tolist(),
                                'excluded_empty_ordinals':np.flatnonzero(~common[offset:offset+size]).tolist(),
                                'singleton_known_rows':sum(g is not None for g in gold),'unknown_or_multi_rows':sum(g is None for g in gold),
                                'discovery':discovery,'group_sizes':dict(group_sizes),'pair_counts':contingency,
                                'labels':descriptors,'annotated_name_token_proxy':{'incidences':len(alignments),'sum':sum(alignments)}})
                timings.append({'meeting':meeting.ordinal,'policy':key,'discovery_ms':discovery_ms,'label_ms':label_ms,
                                'insert_latencies_ms':latencies})
        offset+=size
        if meeting.ordinal%10==0:print(json.dumps({'processed_meetings':meeting.ordinal+1,'turns':offset}),flush=True)
    if offset!=matrices['current'].shape[0]:raise ValueError('incomplete source coverage')
    summary={key:{'pair_metrics':metrics(totals[key]),'groups':len(sizes[key]),'singleton_groups':sum(n==1 for n in sizes[key]),
                  'largest_group':max(sizes[key]),'group_size_median':float(np.median(sizes[key])),
                  'annotated_name_token_proxy_mean':sum(name_scores[key])/len(name_scores[key]),
                  'annotated_name_incidences':len(name_scores[key])} for key in totals}
    result={'schema_version':1,'scope':'meeting_local_insert_only_discovery_not_production_topics',
            'baseline_sha256':BASELINE,'source_sha256':source,'capture_sha256':capture,
            'protocol':{'threshold':.55,'maximum_seeds':2048,'seed':'immutable_original_source_vector',
                        'cosine_tie':'first_created_seed','cutoff_equality':'join','arrival':['forward','reverse'],
                        'representations':['current','context'],'annotation_features':False,
                        'label_counts':'original_source_once','label_method':'ctfidf_times_frozen_source_idf',
                        'label_limit':8,'label_tie':'lexical','stoplist':None,'gold_metrics':'singleton_known_pair_contingency',
                        'name_overlap':'diagnostic_lexical_proxy_not_semantic_quality','new_inference_calls':0,
                        'held_out_sources_read':False,'edits_deletes_supported':False,'multi_tags_supported':False,
                        'semantic_ancestors_supported':False},
            'summary':summary,'records':records,'source_turns':offset,'common_nonempty_turns':int(common.sum()),
            'code_pins':{name:digest(Path(__file__).with_name(name)) for name in
                         ['qmsum_seed_discovery.py','sparse_seed_topics.py','qmsum.py','uv.lock']},
            'timings':{'setup_ms':setup_ms,'runs':timings,'total_after_imports_ms':(time.perf_counter()-start)*1000,
                       'cpu_ms':(time.process_time()-cpu)*1000,'peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}}
    return result


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for key in ['source','baseline','output-directory']:p.add_argument('--'+key,type=Path,required=True)
    p.add_argument('--compare',type=Path);a=p.parse_args()
    if a.output_directory.exists() or a.output_directory.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    a.output_directory.mkdir(parents=True);r=experiment(a)
    if a.compare:compare_capture(json.loads(a.compare.read_text()),r)
    with (a.output_directory/'results.json').open('x') as f:json.dump(r,f,indent=2,allow_nan=False)
    print(json.dumps({'summary':r['summary'],'timings':{k:v for k,v in r['timings'].items() if k!='runs'}}),flush=True)


if __name__=='__main__':main()
