"""Original-space online cosine discovery feasibility and order diagnostics."""
import argparse
from dataclasses import asdict
import hashlib
import json
from pathlib import Path
import platform
import resource
import time

import numpy as np
from sklearn.metrics import adjusted_rand_score

from cqadup_filtered_topics import verified
from cqadup_semantic import digest
from cqadup_topics import SEMANTIC,REPEAT,SAMPLE_SEED
from external_context import compare
from online_cosine import discover,diagnostics


def experiment(first,second,destination):
    started=time.perf_counter();cpu=time.process_time()
    reference=verified(first,second,(SEMANTIC,REPEAT));ids=reference['document_ids']
    if ids!=sorted(set(ids)):raise ValueError('complete canonical source identities required')
    vectors=np.load(first/'balanced-vectors.npy',mmap_mode='r',allow_pickle=False)
    if vectors.shape!=(len(ids),384):raise ValueError('complete original vector dimensions required')
    setup=(time.perf_counter()-started)*1000;files={};captures=[];timings={};partitions=[]
    for order in ['forward','reverse']:
        begin=time.perf_counter();result=discover(vectors,reverse=order=='reverse')
        elapsed=(time.perf_counter()-begin)*1000
        for name,values in [('sums',result.sums),('counts',result.counts),('assignments',result.assignments)]:
            path=destination/(order+'-'+name+'.npy');np.save(path,values,allow_pickle=False);files[path.name]=digest(path)
        captures.append({'order':order,'complete':result.complete,'comparisons':result.comparisons,
                         'diagnostics':diagnostics(vectors,result),'events':[asdict(e) for e in result.events]})
        timings[order]={'discovery_ms':elapsed,'insertion_p50_ms':float(np.median(result.latencies_ms)),
                        'insertion_p95_ms':float(np.percentile(result.latencies_ms,95)),
                        'insertion_max_ms':float(max(result.latencies_ms))}
        partitions.append(result.assignments)
        print(json.dumps({k:v for k,v in captures[-1].items() if k!='events'}),flush=True)
    stability=None
    if all(c['complete'] for c in captures):
        sample=sorted(range(len(ids)),key=lambda i:hashlib.sha256((SAMPLE_SEED+'\0'+ids[i]).encode()).hexdigest())[:512]
        a,b=[x[sample] for x in partitions];mask=np.triu(np.ones((len(sample),len(sample)),dtype=bool),k=1)
        left=(a[:,None]==a[None,:])&mask;right=(b[:,None]==b[None,:])&mask
        union=int((left|right).sum());intersection=int((left&right).sum())
        stability={'full_insertion_ari':float(adjusted_rand_score(*partitions)),
                   'sample_documents':[ids[i] for i in sample],'coassignment_pairs':int(mask.sum()),
                   'intersection':intersection,'union':union,'jaccard':intersection/union if union else None}
    return {'schema_version':1,'scope':'development-online-cosine-discovery-not-semantic-topics',
            'semantic_sha256':SEMANTIC,'semantic_repeat_sha256':REPEAT,'python':platform.python_version(),
            'source_capture_sha256':reference['capture_sha256'],'document_ids':ids,
            'parameters':{'threshold':.55,'maximum_clusters':8192,'metric':'original cosine',
                          'update':'normalized cumulative source sum','tie':'earliest created centroid',
                          'cutoff_equality':'join','passes':1,'reassignment':False},
            'inference_calls':0,'query_outcomes_scored':0,'semantic_ancestors':None,
            'files':files,'captures':captures,'stability':stability,
            'code_pins':{n:digest(Path(__file__).with_name(n)) for n in
                ['online_cosine.py','cqadup_online_cosine.py','topic_routing.py','uv.lock']},
            'timings':{'setup_ms':setup,'orders':timings,'total_ms':(time.perf_counter()-started)*1000,
                       'process_cpu_ms':(time.process_time()-cpu)*1000,
                       'peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}}


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for n in ['semantic','semantic-repeat','output-directory']:p.add_argument('--'+n,type=Path,required=True)
    p.add_argument('--compare',type=Path);a=p.parse_args();dst=a.output_directory
    if dst.exists() or dst.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git capture required')
    dst.mkdir(parents=True);r=json.loads(json.dumps(experiment(a.semantic,a.semantic_repeat,dst),allow_nan=False))
    if a.compare:compare(json.loads(a.compare.read_text()),r)
    with (dst/'results.json').open('x') as f:json.dump(r,f,indent=2,allow_nan=False)
    print(json.dumps({'output':str(dst),'timings':r['timings']}),flush=True)


if __name__=='__main__':main()
