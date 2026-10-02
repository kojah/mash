"""Frozen complete-corpus keyword coherence and reuse; no label intervention."""
import argparse
import json
from pathlib import Path
import platform
import resource
import time

import numpy as np
from scipy import sparse

from cqadup_filtered_topics import FIRST,SECOND,verified
from cqadup_semantic import digest
from external_context import compare
from label_coherence import diagnose,presence_statistics


def experiment(root,repeat):
    started=time.perf_counter();data=verified(root,repeat,(FIRST,SECOND))
    arrays=[np.load(root/('source-counts-'+name+'.npy'),allow_pickle=False) for name in ('data','indices','indptr')]
    counts=sparse.csr_matrix(tuple(arrays),shape=tuple(data['source_counts_shape']))
    vocab=data['vocabulary'];positions={t:i for i,t in enumerate(vocab)}
    selected=sorted({positions[label['term']] for c in data['captures'] for mode in c['modes'].values()
                     for topic in mode['topics'] for labels in topic['labels'].values() for label in labels})
    df,joint=presence_statistics(counts,selected);local={vocab[c]:i for i,c in enumerate(selected)}
    results=[]
    for c in data['captures']:
        for mode,control in c['modes'].items():
            for variant in ('ctfidf','corpus-idf','corpus-idf-diverse'):
                records=[{'topic':i,**diagnose([k['term'] for k in t['labels'][variant]],local,df,joint,counts.shape[0])}
                         for i,t in enumerate(control['topics'])]
                means=[r['mean_npmi'] for r in records if r['mean_npmi'] is not None]
                slots=[t for r in records for t in r['terms']]
                summary={'topics':len(records),'defined_topics':len(means),
                         'macro_npmi':sum(means)/len(means) if len(means)==len(records) and means else None,
                         'label_slots':len(slots),'unique_terms':len(set(slots)),
                         'unique_term_ratio':len(set(slots))/len(slots) if slots else None,
                         'pairs':sum(len(r['pairs']) for r in records),
                         'zero_joint_pairs':sum(r['zero_joint_pairs'] for r in records),
                         'undefined_pairs':sum(r['undefined_pairs'] for r in records),
                         'containment_pairs':sum(r['containment_pairs'] for r in records)}
                results.append({'order':c['order'],'mode':mode,'variant':variant,'records':records,'summary':summary})
                print(json.dumps({k:v for k,v in results[-1].items() if k!='records'}),flush=True)
    return {'schema_version':1,'scope':'development-label-document-coherence-not-semantic-gold',
            'topic_sha256':FIRST,'topic_repeat_sha256':SECOND,'python':platform.python_version(),
            'parameters':{'estimator':'whole original candidate document binary presence','documents':counts.shape[0],
                          'zero_joint':-1,'joint_equals_documents':None,'smoothing':False,'label_limit':8},
            'selected_columns':selected,'selected_terms':[vocab[c] for c in selected],
            'document_frequencies':df.tolist(),'joint_frequencies':joint.tolist(),
            'inference_calls':0,'reserved_queries_scored':0,'results':results,
            'code_pins':{n:digest(Path(__file__).with_name(n)) for n in
                         ('cqadup_label_coherence.py','label_coherence.py','cqadup_filtered_topics.py',
                          'cqadup_semantic.py','external_context.py','uv.lock')},
            'timings':{'total_ms':(time.perf_counter()-started)*1000,
                       'peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ('topics','topic-repeat','output'): parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--compare',type=Path);args=parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git output required')
    result=json.loads(json.dumps(experiment(args.topics,args.topic_repeat),allow_nan=False))
    if args.compare: compare(json.loads(args.compare.read_text()),result)
    with args.output.open('x') as f: json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps({'output':str(args.output),'total_ms':result['timings']['total_ms']}),flush=True)


if __name__=='__main__': main()
