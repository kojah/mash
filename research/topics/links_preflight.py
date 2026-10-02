"""Pinned Links threshold reference and candidate-only geometry diagnostic."""
import argparse
from dataclasses import asdict
import hashlib
import importlib
import json
from pathlib import Path
import platform
import sys
import time

import numpy as np

from cqadup_filtered_topics import verified
from cqadup_semantic import digest
from cqadup_topics import SEMANTIC,REPEAT,SAMPLE_SEED
from external_context import compare
from links_thresholds import Parameters
from topic_routing import unit_matrix


AUTHOR='33faf171cf145a1e0a8b35d75730db5aca22baf1'
MANIFEST='c30686369754137063394c219edfdec904b1610b34655c55a5bf3b6ba4b0ac05'


def experiment(first,second,author):
    started=time.perf_counter();manifest=author/'source-manifest.json'
    if digest(manifest)!=MANIFEST:raise ValueError('pinned author manifest required')
    m=json.loads(manifest.read_text())
    if m['commit']!=AUTHOR or m['repository']!='https://github.com/wq2012/LinksCluster':raise ValueError('author source identity drift')
    for name,sha in m['files'].items():
        if digest(author/name)!=sha:raise ValueError('author source bytes changed')
    # Explicit verified local reference, not a package install or downloaded model.
    sys.path.insert(0,str(author.resolve()));reference=importlib.import_module('linkscluster')
    if Path(reference.__file__).resolve()!=author.resolve()/'linkscluster/__init__.py':raise ValueError('wrong imported reference')
    parameters=Parameters();model=reference.LinksClusterer(tc=parameters.tc,ts=parameters.ts,tp=parameters.tp)
    records=[]
    for k in [1,2,10,100,1000]:
        for other in [1,2,10,100,1000]:
            value=parameters.pair(k,other);expected=model.s_tilde(k,other)
            if abs(value-expected)>1e-15:raise ValueError('threshold formula mismatch')
            records.append({'k':k,'other':other,'threshold':value,'reference':expected})
    data=verified(first,second,(SEMANTIC,REPEAT));ids=data['document_ids']
    if ids!=sorted(set(ids)):raise ValueError('canonical complete candidate identities required')
    vectors=unit_matrix(np.load(first/'balanced-vectors.npy',mmap_mode='r',allow_pickle=False))
    if vectors.shape!=(len(ids),384):raise ValueError('complete original candidate vectors required')
    sample=sorted(range(len(ids)),key=lambda i:hashlib.sha256((SAMPLE_SEED+'\0'+ids[i]).encode()).hexdigest())[:512]
    x=vectors[sample];scores=x@x.T;mask=np.triu(np.ones(scores.shape,dtype=bool),k=1);pairs=scores[mask]
    total=vectors.sum(axis=0)
    return {'schema_version':1,'scope':'candidate-geometry-and-formula-only-not-Links-clustering',
            'python':platform.python_version(),'author_commit':AUTHOR,'author_manifest_sha256':MANIFEST,
            'semantic_sha256':SEMANTIC,'semantic_repeat_sha256':REPEAT,
            'parameters':asdict(parameters),'reference_thresholds':records,
            'candidate_documents':len(ids),'sample_documents':[ids[i] for i in sample],
            'sample_pairs':len(pairs),'sample_cosine_p05':float(np.percentile(pairs,5)),
            'sample_cosine_median':float(np.median(pairs)),'sample_cosine_p95':float(np.percentile(pairs,95)),
            'pairs_ge_singleton_link_cutoff':int((pairs>=parameters.pair(1)).sum()),
            'pairs_ge_subcluster_cutoff':int((pairs>=parameters.ts).sum()),
            'complete_mean_direction_norm':float(np.linalg.norm(total/len(ids))),
            'complete_mean_pair_cosine':float((total@total-len(ids))/(len(ids)*(len(ids)-1))),
            'inference_calls':0,'cluster_outcomes':None,'query_outcomes_scored':0,
            'code_pins':{n:digest(Path(__file__).with_name(n)) for n in ['links_preflight.py','links_thresholds.py','uv.lock']},
            'timings':{'total_ms':(time.perf_counter()-started)*1000}}


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for n in ['semantic','semantic-repeat','author','output']:p.add_argument('--'+n,type=Path,required=True)
    p.add_argument('--compare',type=Path);a=p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git capture required')
    result=json.loads(json.dumps(experiment(a.semantic,a.semantic_repeat,a.author),allow_nan=False))
    if a.compare:compare(json.loads(a.compare.read_text()),result)
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps({k:v for k,v in result.items() if k not in ['sample_documents','reference_thresholds','code_pins']}),flush=True)


if __name__=='__main__':main()
