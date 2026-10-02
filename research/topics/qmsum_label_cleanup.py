"""Fixed-membership, source-preserving label-only cleanup comparison."""
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
from label_noise import ACKNOWLEDGEMENTS,EXCLUDED,Policy,clean
from qmsum import load,membership
from sparse_seed_topics import labels


DISCOVERY='83d0b902b1833218a8be86b9888c35702e86808f5886a469facd369f8e1cc3d6'
BASELINE='e3c89677f538f65f3e23a5a7de14de6ef9340febb40a408d97ebb275cb11b488'
TOKEN=re.compile(r'(?u)\b\w\w+\b')


def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()


def save_matrix(root,name,matrix):
    files={}
    for key,value in [('data',matrix.data),('indices',matrix.indices),('indptr',matrix.indptr)]:
        path=root/f'{name}-{key}.npy';np.save(path,value,allow_pickle=False);files[path.name]=digest(path)
    return {'shape':list(matrix.shape),'nnz':matrix.nnz,'files':files}


def experiment(a):
    start=time.perf_counter();cpu=time.process_time()
    if digest(a.discovery)!=DISCOVERY or digest(a.baseline/'results.json')!=BASELINE:raise ValueError('audited fixed evidence required')
    discovery=json.loads(a.discovery.read_text());baseline=json.loads((a.baseline/'results.json').read_text())
    capture,source,meetings=load(a.source)
    if (capture,source)!=(discovery['capture_sha256'],discovery['source_sha256']):raise ValueError('original source drift')
    for name in ['sparse_seed_topics.py','qmsum.py','uv.lock']:
        if digest(Path(__file__).with_name(name))!=discovery['code_pins'][name]:raise ValueError('frozen label/source implementation drift')
    d=baseline['matrices']['counts']
    for filename,pin in d['files'].items():
        if digest(a.baseline/filename)!=pin:raise ValueError('count drift')
    arrays=[np.load(a.baseline/f'counts-{k}.npy',allow_pickle=False) for k in ['data','indices','indptr']]
    counts=sparse.csr_matrix(tuple(arrays),shape=tuple(d['shape']))
    if digest(a.baseline/'vocabulary.json')!=baseline['vocabulary_sha256'] or digest(a.baseline/'idf.npy')!=baseline['idf_sha256']:raise ValueError('vocabulary/IDF drift')
    terms=json.loads((a.baseline/'vocabulary.json').read_text());idf=np.load(a.baseline/'idf.npy')
    texts=[t.text for m in meetings for t in m.turns];setup_ms=(time.perf_counter()-start)*1000;begin=time.perf_counter()
    variants,ledger=clean(counts,texts,terms);cleanup_ms=(time.perf_counter()-begin)*1000
    matrix_captures={p:save_matrix(a.output_directory,p,m) for p,m in variants.items()}
    row_coverage={p:{'nonempty_sources':int((m.getnnz(axis=1)>0).sum()),'total_source_counts':int(m.sum()),
                     'removed_counts':int(counts.sum()-m.sum())} for p,m in variants.items()}
    frozen={(r['meeting'],r['policy']):r for r in discovery['records']};records=[];timings=[];summary={};offset=0
    for key in discovery['summary']:
        for policy in Policy:summary[key+':'+policy]=Counter()
    for meeting in meetings:
        size=len(meeting.turns);members,_=membership(meeting);names=[set(TOKEN.findall(t.name.lower())) for t in meeting.topics]
        for key in discovery['summary']:
            original=frozen[(meeting.ordinal,key)];positions=np.array(original['source_ordinals']);assigned=original['discovery']['assignments']
            for policy,matrix in variants.items():
                begin=time.perf_counter();descriptors=labels(matrix[offset+positions],assigned,idf,terms)
                elapsed=(time.perf_counter()-begin)*1000
                if policy==Policy.RAW and descriptors!=original['labels']:raise ValueError('original label ranking drift')
                coverage=Counter({'groups':len(descriptors),'empty_groups':sum(not d for d in descriptors),
                                  'descriptor_terms':sum(len(d) for d in descriptors),
                                  'excluded_vocabulary_descriptor_terms':sum(t['term'] in EXCLUDED for d in descriptors for t in d),
                                  'marker_named_descriptor_terms':sum(t['term'] in {'disfmarker','vocalsound'} for d in descriptors for t in d)})
                alignment=[]
                for ti,name in enumerate(names):
                    for j,i in enumerate(positions):
                        if ti in members[i]:alignment.append(len(name&{t['term'] for t in descriptors[assigned[j]]})/len(name))
                coverage['annotated_name_incidences']=len(alignment);coverage['annotated_name_proxy_sum']=sum(alignment)
                summary[key+':'+policy].update(coverage)
                records.append({'meeting':meeting.ordinal,'discovery_policy':key,'label_policy':policy,
                                'ownership_sha256':hashlib.sha256(json.dumps({'source_ordinals':positions.tolist(),'assignments':assigned},sort_keys=True).encode()).hexdigest(),
                                'labels':descriptors,'coverage':dict(coverage)})
                timings.append({'meeting':meeting.ordinal,'discovery_policy':key,'label_policy':policy,'label_ms':elapsed})
        offset+=size
        if meeting.ordinal%25==0:print(json.dumps({'processed_meetings':meeting.ordinal+1}),flush=True)
    if offset!=len(texts) or len(frozen)!=len(meetings)*4:raise ValueError('incomplete fixed source coverage')
    summarized={k:{**dict(v),'annotated_name_token_proxy_mean':v['annotated_name_proxy_sum']/v['annotated_name_incidences']} for k,v in summary.items()}
    return {'schema_version':1,'scope':'fixed_membership_label_only_not_semantic_quality',
            'source_sha256':source,'capture_sha256':capture,'discovery_sha256':DISCOVERY,'baseline_sha256':BASELINE,
            'protocol':{'policies':list(Policy),'exact_markers':['{disfmarker}','{vocalsound}'],'marker_match':'case_sensitive',
                        'acknowledgements':sorted(ACKNOWLEDGEMENTS),'excluded_label_vocabulary':sorted(EXCLUDED),
                        'stoplist_origin':'sklearn1.9.1_ENGLISH_STOP_WORDS','source_idf':'unchanged_original_full_train',
                        'class_statistics':'recomputed_for_each_label_count_policy','source_text_modified':False,
                        'memberships_changed':False,'embeddings_changed':False,'annotated_name_denominator':'unchanged_raw_source_name_tokens',
                        'empty_groups':'retained_without_invented_labels','new_inference_calls':0,'held_out_sources_read':False},
            'matrices':matrix_captures,'row_coverage':row_coverage,'source_turns':len(texts),'marker_ledger':ledger,
            'marker_counts':dict(Counter(x['term'] for x in ledger)),'records':records,'summary':summarized,
            'code_pins':{name:digest(Path(__file__).with_name(name)) for name in
                         ['qmsum_label_cleanup.py','label_noise.py','sparse_seed_topics.py','qmsum.py','uv.lock']},
            'timings':{'setup_ms':setup_ms,'cleanup_ms':cleanup_ms,'labels':timings,'total_after_imports_ms':(time.perf_counter()-start)*1000,
                       'cpu_ms':(time.process_time()-cpu)*1000,'peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}}


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for key in ['source','baseline','discovery','output-directory']:p.add_argument('--'+key,type=Path,required=True)
    p.add_argument('--compare',type=Path);a=p.parse_args()
    if a.output_directory.exists() or a.output_directory.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    a.output_directory.mkdir(parents=True);r=experiment(a)
    if a.compare:compare(json.loads(a.compare.read_text()),json.loads(json.dumps(r,allow_nan=False)))
    with (a.output_directory/'results.json').open('x') as f:json.dump(r,f,indent=2,allow_nan=False)
    print(json.dumps({'summary':r['summary'],'row_coverage':r['row_coverage'],'timings':{k:v for k,v in r['timings'].items() if k!='labels'}}),flush=True)


if __name__=='__main__':main()
