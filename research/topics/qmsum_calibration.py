"""One bounded forward-bootstrap model, then fixed weighted online coordinates."""
import argparse
from collections import Counter
import json
import math
from pathlib import Path
import resource
import time
from types import SimpleNamespace
import numpy as np
from scipy import sparse
from threadpoolctl import threadpool_limits
from block_hierarchy import evaluate
from calibration_metrics import compact,post_bootstrap,topology
from calibrated_frame import calibrate,prepare
from discovery_inputs import read,digest
from episode_blocks import block_counts
from online_tree import Mode
from qmsum_block_hierarchy import stored_tree
from qmsum_compressed_macro import REFERENCE
from qmsum_hierarchy import components
from qmsum_sparse_average import fit
from qmsum_unique_blocks import GEOMETRY
from stable_frame_quality import cosine_errors,label_examples
from stable_lexical_frame import Policy


TF_REFERENCE='722d9acb930c8e0bd6df163c5ad81687f88df73a826da46b1a0f1e0ea79ee741'
PINNAMES=['qmsum_calibration.py','CALIBRATION_PROTOCOL.md','calibrated_frame.py','calibration_metrics.py','stable_lexical_frame.py',
    'stable_frame_quality.py','sparse_average_tree.py','qmsum_sparse_average.py','discovery_inputs.py','episode_blocks.py','block_hierarchy.py','captured_hierarchy.py',
    'online_tree.py','data.py','qmsum.py','qmsum_hierarchy.py','topic_labels.py','subject_labels.py','label_noise.py','uv.lock']


def experiment(root,output):
    start=time.perf_counter();cpu=time.process_time()
    code_pins={n:digest(Path(__file__).with_name(n)) for n in PINNAMES}
    def folder(n):return root/f'topic-qmsum-{n}-20261002'
    gpath=folder('block-geometry-first')/'results.json';ref=root/'topic-qmsum-block-hierarchy-final-first-20261002.json';tf=root/'topic-qmsum-stable-frame-quality-first-20261002.json'
    if digest(gpath)!=GEOMETRY or digest(ref)!=REFERENCE or digest(tf)!=TF_REFERENCE:raise ValueError('immutable source controls required')
    geometry=json.loads(gpath.read_text());reference=json.loads(ref.read_text());tf_reference=json.loads(tf.read_text())
    args=SimpleNamespace(source=folder('author-training'),vectors=folder('semantic-vectors-first'),units=folder('semantic-units-final-first'),
        labels=folder('label-cleanup-first'),baseline=folder('lexical-context-final-first'),reference_manifest=root/'grinch-reference-20261001/manifest.json')
    data=read(args)
    for name,pin in geometry['code_pins'].items():
        if digest(Path(__file__).with_name(name))!=pin:raise ValueError('frozen source geometry drift')
    if any(c['source_sha256']!=data.source for c in [geometry,reference,tf_reference]):raise ValueError('original source drift')
    counts=[];owners=[];seed=[];ids=[]
    for meeting,row in zip(data.meetings,geometry['records']):
        owner=np.full(len(meeting.turns),-1,dtype=np.int64)
        for b in row['blocks']:owner[b['start']:b['end']+1]=b['ordinal']
        if not np.array_equal(owner>=0,data.ready[row['source_offset']:row['source_offset']+len(owner)]):raise ValueError('source support drift')
        bank=block_counts(data.counts[row['source_offset']:row['source_offset']+len(owner)],owner,len(row['blocks']));counts.append(bank);owners.append(owner)
        for i in range(len(row['blocks'])):
            if len(seed)==32:break
            r=bank[i];seed.append({data.terms[j]:int(v) for j,v in zip(r.indices,r.data)})
            ids.append(f'{data.source}/meeting:{meeting.ordinal}/block:{i}')
    if len(seed)!=32:raise ValueError('full bounded bootstrap required')
    setup_ms=(time.perf_counter()-start)*1000;begin=time.perf_counter();models={p:calibrate(seed,ids,p) for p in Policy}
    model_ms=(time.perf_counter()-begin)*1000;model_cpu=time.process_time();begin=time.perf_counter();banks={};states={};files={}
    for p in Policy:
        for kind in ['hashed','identity_oracle']:
            encoder=prepare(models[p],None if kind=='hashed' else data.terms);vectors=[];local_states=[]
            for bank in counts:
                for i in range(bank.shape[0]):
                    r=bank[i];row=encoder.encode({data.terms[j]:int(v) for j,v in zip(r.indices,r.data)})
                    if row.direction is None:raise ValueError('original complete nonempty block support required')
                    vectors.append(row.direction);local_states.append(row.state)
            v=sparse.vstack(vectors,format='csr');banks[p,kind]=v;states[p,kind]=local_states
            for key,array in [('data',v.data),('indices',v.indices),('indptr',v.indptr)]:
                path=output/f'{p}-{kind}-{key}.npy';np.save(path,array,allow_pickle=False);files[path.name]=digest(path)
    feature_ms=(time.perf_counter()-begin)*1000;feature_cpu_ms=(time.process_time()-model_cpu)*1000
    records=[];timings=[];distortions=[];matched_baselines=[];support=Counter()
    with threadpool_limits(limits=1):
        for meeting,row,owner,bank in zip(data.meetings,geometry['records'],owners,counts):
            selected=np.flatnonzero(owner>=0).tolist();gold,parts=components(meeting,selected);owned=owner[selected].tolist();lo=row['block_directions_offset'];n=len(row['blocks']);threshold=max(0,32-lo)
            offline=next(r for r in reference['records'] if r['meeting']==meeting.ordinal and r['method']=='lexical_average')
            matched_baselines.append({'meeting':meeting.ordinal,'policy':'offline_tfidf','kind':'offline_average','order':'offline',
                'full':compact(offline['metrics']),'post_bootstrap':post_bootstrap(topology(offline['tree']),owned,gold,parts,selected,threshold) if threshold else compact(offline['metrics'])})
            for old in [r for r in tf_reference['records'] if r['meeting']==meeting.ordinal]:
                matched_baselines.append({'meeting':meeting.ordinal,'policy':old['policy'],'kind':old['kind'],'order':old['order'],
                    'full':compact(old['metrics']),'post_bootstrap':post_bootstrap(topology(old['tree']),owned,gold,parts,selected,threshold) if threshold else compact(old['metrics'])})
            for p in Policy:
                h=banks[p,'hashed'][lo:lo+n];identity=banks[p,'identity_oracle'][lo:lo+n]
                distortions.append({'meeting':meeting.ordinal,'policy':p,**cosine_errors(h,identity)})
                for kind in ['hashed','identity_oracle']:
                    vectors=banks[p,kind][lo:lo+n];local_states=states[p,kind][lo:lo+n]
                    for order,reverse in [('forward',False),('reverse',True)]:
                        tree,state,t=fit(vectors,'lexical',Mode.ROTATE,reverse);begin=time.perf_counter()
                        metrics=evaluate(tree,owned,gold,parts,selected,len(meeting.topics));post=post_bootstrap(tree,owned,gold,parts,selected,threshold) if threshold else compact(metrics)
                        t['evaluation_ms']=(time.perf_counter()-begin)*1000;begin=time.perf_counter();labels=label_examples(tree,vectors,bank,data.terms,local_states);t['labels_ms']=(time.perf_counter()-begin)*1000
                        for label in labels:
                            b=row['blocks'][label['representative_block']]
                            label['representative_source']={'start':b['start'],'end':b['end'],'text':'\n'.join(turn.text for turn in meeting.turns[b['start']:b['end']+1])}
                        records.append({'meeting':meeting.ordinal,'policy':p,'kind':kind,'order':order,'tree':stored_tree(tree),'state':state,
                            'states':local_states,'blocks':n,'metrics':metrics,'post_bootstrap':post,'display_labels':labels})
                        timings.append({'meeting':meeting.ordinal,'policy':p,'kind':kind,'order':order,**t})
            support.update(sources=len(owner),ready=len(selected),known=sum(g is not None for g in gold),blocks=n)
            if meeting.ordinal%10==0:print(json.dumps({'processed_meetings':meeting.ordinal+1}),flush=True)
    if support!={'sources':88336,'ready':84434,'known':77892,'blocks':8196}:raise ValueError('unchanged full original source required')
    summaries={}
    for p in Policy:
        summaries[p]={}
        for kind in ['hashed','identity_oracle']:
            summaries[p][kind]={}
            for order in ['forward','reverse']:
                chosen=[r for r in records if (r['policy'],r['kind'],r['order'])==(p,kind,order)];entry={}
                for phase in ['full','post_bootstrap']:
                    entry[phase]={}
                    for scope in ['all','far','crossspan']:
                        values=[r['metrics' if phase=='full' else phase][scope] for r in chosen];pairs=sum(v['pairs'] for v in values);weight=math.fsum(v['purity_sum'] for v in values)
                        bases=[r[phase][scope] for r in matched_baselines if r['policy']=='offline_tfidf'];base_pairs=sum(v['pairs'] for v in bases);base_weight=math.fsum(v['purity_sum'] for v in bases)
                        if pairs!=base_pairs:raise ValueError('matched original baseline pair support required')
                        entry[phase][scope]={'pairs':pairs,'purity_sum':weight,'purity':weight/pairs,'offline_reference':base_weight/pairs,'loss_vs_offline':(base_weight-weight)/pairs}
                summaries[p][kind][order]=entry
    if code_pins!={n:digest(Path(__file__).with_name(n)) for n in PINNAMES}:raise ValueError('source changed during experiment')
    return {'schema_version':1,'scope':'bounded_global_prefix_calibration_not_production_selection','source_sha256':data.source,'source_capture_sha256':data.capture,
        'geometry_sha256':GEOMETRY,'offline_reference_sha256':REFERENCE,'tf_reference_sha256':TF_REFERENCE,'support':dict(support),
        'models':{p:models[p].description() for p in Policy},'bootstrap_identities':ids,'files':files,'summary':summaries,
        'records':records,'matched_baselines':matched_baselines,'cosine_distortions':distortions,
        'protocol':{'bootstrap_blocks':32,'new_inference':0,'llm_calls':0,'heldout_read':False,'production_selection':False,'full_ingestion_gate':False,
            'calibration_is_forward_in_both_insertion_orders':True,'future_idf_loaded_only_for_frozen_input_validation_not_features':True,'semantic_cut_improved':False},
        'code_pins':code_pins,
        'timings':{'setup_ms':setup_ms,'model_ms':model_ms,'feature_ms':feature_ms,'feature_cpu_ms':feature_cpu_ms,
            'after_imports_ms':(time.perf_counter()-start)*1000,'cpu_ms':(time.process_time()-cpu)*1000,'rusage_peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,'stages':timings}}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--artifacts',type=Path,required=True);p.add_argument('--output-directory',type=Path,required=True);p.add_argument('--compare',type=Path);a=p.parse_args()
    if a.output_directory.exists() or a.output_directory.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    a.output_directory.mkdir();r=experiment(a.artifacts,a.output_directory)
    if a.compare:
        old=json.loads((a.compare/'results.json').read_text())
        if {k:v for k,v in old.items() if k!='timings'}!={k:v for k,v in r.items() if k!='timings'}:raise ValueError('complete non-timing repeat differs')
    with (a.output_directory/'results.json').open('x') as f:json.dump(r,f,indent=2,allow_nan=False)
    print(json.dumps({'summary':r['summary'],'support':r['support']}),flush=True)


if __name__=='__main__':main()
