"""Bounded complete-input Links reference trial; failed coverage stays explicit."""
import argparse
import json
from pathlib import Path
import platform
import resource
import time

import numpy as np

from cqadup_filtered_topics import verified
from cqadup_semantic import digest
from cqadup_topics import SEMANTIC,REPEAT
from external_context import compare
from links_reference import AUTHOR,MANIFEST,load
from links_state import snapshot
from links_trial import run


def experiment(first,second,author,destination):
    started=time.perf_counter();cpu=time.process_time();reference=load(author)
    data=verified(first,second,(SEMANTIC,REPEAT));ids=data['document_ids']
    if ids!=sorted(set(ids)):raise ValueError('complete canonical candidates required')
    vectors=np.load(first/'balanced-vectors.npy',mmap_mode='r',allow_pickle=False)
    if vectors.shape!=(len(ids),384):raise ValueError('complete original vectors required')
    setup=(time.perf_counter()-started)*1000;captures=[];files={};timings={}
    for order in ['forward','reverse']:
        begin=time.perf_counter();r=run(reference,vectors,reverse=order=='reverse');discovery=(time.perf_counter()-begin)*1000
        s=snapshot(r.model,vectors,r.arrivals)
        online=np.full(len(ids),-1,dtype=np.int64);online[r.arrivals]=r.model.get_online_labels()
        for name,values in [('sums',s.sums),('counts',s.counts),('final-labels',s.labels),('online-labels',online)]:
            path=destination/(order+'-'+name+'.npy');np.save(path,values,allow_pickle=False);files[path.name]=digest(path)
        sizes=np.unique(s.labels[s.labels>=0],return_counts=True)[1]
        summary={'covered':len(r.arrivals),'unprocessed':len(ids)-len(r.arrivals),
                 'subclusters':len(s.node_ids),'clusters':len(sizes),'edges':len(s.edges),
                 'singleton_subclusters':int((s.counts==1).sum()),'subcluster_max_size':int(s.counts.max()),
                 'component_max_size':int(sizes.max()),'component_median_size':float(np.median(sizes)),
                 'online_label_changes':int((s.labels[r.arrivals]!=online[r.arrivals]).sum())}
        captures.append({'order':order,'complete':r.complete,'budget':8192,
                         'attempted_document':ids[r.attempted] if r.attempted is not None else None,
                         'summary':summary,'node_ids':s.node_ids,'cluster_ids':s.cluster_ids,
                         'members':s.members,'edges':s.edges,'events':r.events,
                         'graph_events':r.graph_events,'checkpoints':r.checkpoints,
                         'quality_metrics':None,'semantic_ancestors':None})
        timings[order]={'guarded_discovery_ms':discovery,'insertion_p50_ms':float(np.median(r.latencies_ms)),
                        'insertion_p95_ms':float(np.percentile(r.latencies_ms,95))}
        print(json.dumps({'order':order,'complete':r.complete,'summary':summary}),flush=True)
    return {'schema_version':1,'scope':'budgeted-Links-reference-not-topic-selection',
            'python':platform.python_version(),'author_commit':AUTHOR,'author_manifest_sha256':MANIFEST,
            'semantic_sha256':SEMANTIC,'semantic_repeat_sha256':REPEAT,'document_ids':ids,
            'parameters':{'tc':.5,'ts':.8,'tp':1.,'maximum_subclusters':8192,
                          'normalize_input':False,'use_theoretical_norm':False,
                          'snapshot_interval':256,'maximum_snapshot_edges':16384},
            'inference_calls':0,'query_outcomes_scored':0,'files':files,'captures':captures,
            'code_pins':{n:digest(Path(__file__).with_name(n)) for n in
                ['links_reference.py','links_trial.py','links_state.py','cqadup_links_trial.py','uv.lock']},
            'timings':{'setup_ms':setup,'orders':timings,'total_ms':(time.perf_counter()-started)*1000,
                       'process_cpu_ms':(time.process_time()-cpu)*1000,
                       'peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}}


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for n in ['semantic','semantic-repeat','author','output-directory']:p.add_argument('--'+n,type=Path,required=True)
    p.add_argument('--compare',type=Path);a=p.parse_args();dst=a.output_directory
    if dst.exists() or dst.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    dst.mkdir(parents=True);r=json.loads(json.dumps(experiment(a.semantic,a.semantic_repeat,a.author,dst),allow_nan=False))
    if a.compare:compare(json.loads(a.compare.read_text()),r)
    with (dst/'results.json').open('x') as f:json.dump(r,f,indent=2,allow_nan=False)
    print(json.dumps({'output':str(dst),'timings':r['timings']}),flush=True)


if __name__=='__main__':main()
