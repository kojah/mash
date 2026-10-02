"""Merged versus title-only evidence on frozen vocabulary and topic memberships."""
import argparse
import json
from pathlib import Path
import platform
import resource
import time

import numpy as np

from cqadup_filtered_topics import FIRST,SECOND,verified
from cqadup_semantic import digest
from cqadup_semantic_labels import csr
from cqadup_topics import save_sparse
from external_context import compare
from label_coherence import diagnose,presence_statistics
from topic_field_evidence import Field,candidates,counts
from topic_labels import classes,labels,support
from topic_routing import Match


def experiment(topics,repeat,source,destination):
    started=time.perf_counter();cpu_started=time.process_time();data=verified(topics,repeat,(FIRST,SECOND))
    docs=candidates(source,data['source_capture_sha256'],data['document_ids']);terms=data['vocabulary'];positions={t:i for i,t in enumerate(terms)}
    background=csr(topics,'source-counts',data['source_counts_shape']);files={}
    begin=time.perf_counter();title=counts(docs,terms,Field.TITLE);body=counts(docs,terms,Field.BODY)
    if (title+body-background).nnz: raise ValueError('complete field count conservation failed')
    for name,matrix in [('title',title),('body',body)]:save_sparse(matrix,name+'-counts',destination,files)
    field_ms=(time.perf_counter()-begin)*1000;setup_ms=(time.perf_counter()-started)*1000
    results=[]
    for c in data['captures']:
        for mode,control in c['modes'].items():
            members=[tuple(Match(**m) for m in row) for row in control['memberships']]
            for name,evidence in [('merged',background),('title',title)]:
                aggregate=classes(evidence,members,len(c['topic_ids']));descriptions=labels(aggregate,background,terms)
                if name=='merged' and descriptions!=[t['labels'] for t in control['topics']]:raise ValueError('frozen merged labels changed')
                shape=save_sparse(aggregate,c['order']+'-'+mode+'-'+name+'-classes',destination,files)
                inverted=evidence.tocsc();records=[]
                for i,descriptor in enumerate(descriptions):
                    member_ids={j for j,row in enumerate(members) if any(m.topic==i for m in row)}
                    supports={}
                    for term in sorted({k['term'] for rank in descriptor.values() for k in rank}):
                        col=positions[term];owners=[int(j) for j in inverted.indices[inverted.indptr[col]:inverted.indptr[col+1]] if int(j) in member_ids]
                        if not owners:raise ValueError('label lacks selected-field source evidence')
                        span=support(docs[owners[0]],term)
                        if name=='title' and span['field']!='title':raise ValueError('title label borrowed body evidence')
                        supports[term]={'supporting_documents':len(owners),'first_source':span}
                    records.append({'topic':i,'labels':descriptor,'source_support':supports})
                results.append({'order':c['order'],'mode':mode,'field':name,'class_shape':shape,
                    'class_nnz':aggregate.nnz,'class_mass':float(aggregate.sum()),
                    'zero_evidence_topics':np.flatnonzero(np.asarray(aggregate.sum(axis=1)).ravel()==0).tolist(),
                    'records':records})
    selected=sorted({positions[k['term']] for r in results for row in r['records'] for rank in row['labels'].values() for k in rank})
    df,joint=presence_statistics(background,selected);local={terms[c]:i for i,c in enumerate(selected)};summaries=[]
    for r in results:
        for variant in ('ctfidf','corpus-idf','corpus-idf-diverse'):
            diagnostics=[{'topic':row['topic'],**diagnose([k['term'] for k in row['labels'][variant]],local,df,joint,len(docs))} for row in r['records']]
            means=[v['mean_npmi'] for v in diagnostics if v['mean_npmi'] is not None];slots=[t for d in diagnostics for t in d['terms']]
            summaries.append({k:r[k] for k in ('order','mode','field')}|{'variant':variant,
                'topics':len(diagnostics),'defined_topics':len(means),'label_slots':len(slots),
                'macro_npmi':float(np.mean(means)) if len(means)==len(diagnostics) and means else None,
                'unique_term_ratio':len(set(slots))/len(slots) if slots else None,'diagnostics':diagnostics})
            print(json.dumps({k:v for k,v in summaries[-1].items() if k!='diagnostics'}),flush=True)
    return {'schema_version':1,'scope':'development-label-field-evidence-not-semantic-quality',
            'topic_sha256':FIRST,'topic_repeat_sha256':SECOND,'source_capture_sha256':data['source_capture_sha256'],
            'python':platform.python_version(),'document_ids':data['document_ids'],
            'parameters':{'vocabulary':'frozen merged candidate vocabulary','background_idf':'original merged document presence',
                          'labels':8,'class_mass':'equal assigned document mass over retained tags','source_token_rules':'unchanged'},
            'field_shapes':{'title':list(title.shape),'body':list(body.shape)},'field_nnz':{'title':title.nnz,'body':body.nnz},
            'field_mass':{'title':float(title.sum()),'body':float(body.sum())},
            'zero_title_document_ids':[docs[i].id for i in np.flatnonzero(np.diff(title.indptr)==0)],
            'inference_calls':0,'reserved_queries_scored':0,'files':files,'results':results,'summary':summaries,
            'code_pins':{n:digest(Path(__file__).with_name(n)) for n in
                ('cqadup_field_labels.py','topic_field_evidence.py','topic_labels.py','cqadup_semantic_labels.py',
                 'cqadup_source.py','subject_labels.py','label_coherence.py','cqadup_topics.py','uv.lock')},
            'timings':{'field_count_ms':field_ms,'setup_ms':setup_ms,'total_ms':(time.perf_counter()-started)*1000,
                       'process_cpu_ms':(time.process_time()-cpu_started)*1000,'peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for n in ('topics','topic-repeat','source','output-directory'):parser.add_argument('--'+n,type=Path,required=True)
    parser.add_argument('--compare',type=Path);args=parser.parse_args();dst=args.output_directory
    if dst.exists() or dst.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    dst.mkdir(parents=True);result=json.loads(json.dumps(experiment(args.topics,args.topic_repeat,args.source,dst),allow_nan=False))
    if args.compare:compare(json.loads(args.compare.read_text()),result)
    with (dst/'results.json').open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps({'output':str(dst),'timings':result['timings']}),flush=True)


if __name__=='__main__':main()
