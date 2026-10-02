"""Apply sealed training cutoffs to author development data; no calibration here."""
import argparse
from dataclasses import asdict
import json
from pathlib import Path
import platform
import resource
import time
import urllib.request

import numpy as np
from sklearn.metrics import average_precision_score,roc_auc_score
from tokenizers import Tokenizer

from boundary_scoring import Method,confusion,score_turns
from external_context import compare
from local_embeddings import Encoder
from tiage import Shift,load,parse
from tiage_calibration import MODEL,TOKENIZER,bootstrap,digest
from tiage_capture import COMMIT,verify


FIRST='652173df490d2a93882ad81165dfb9affd6642acff898da839d9004da4e53ea0'
REPEAT='2f7aca3a331b46dae8f023cc5700139fcf86e72703435950fcfe711ba3a1f0bb'
PATH='data/personachat/anno/dev/anno_dev.json'
BLOB='c1f3c9374ed2a27979b13126b46998526543a302'
SIZE=96155


def paired_baseline(records):
    matrix=np.array([[r[k] for k in ['tp','fp','fn','tn']] for r in records])
    rng=np.random.default_rng(20261002);totals=matrix[rng.integers(0,len(matrix),size=(1000,len(matrix)))].sum(axis=1)
    den=2*totals[:,0]+totals[:,1]+totals[:,2]
    f1=np.divide(2*totals[:,0],den,out=np.zeros(1000,dtype=float),where=den>0)
    positives=totals[:,0]+totals[:,2];all_den=2*positives+totals[:,1]+totals[:,3]
    all_change=np.divide(2*positives,all_den,out=np.zeros(1000,dtype=float),where=all_den>0)
    return {'draws':1000,'seed':20261002,'f1_minus_all_change_percentiles':np.percentile(f1-all_change,[2.5,50,97.5]).tolist(),
            'limitation':'descriptive dialogue resampling, not calibrated significance or coding-topic evidence'}


def sealed(first,second):
    if digest(first/'results.json')!=FIRST or digest(second/'results.json')!=REPEAT:
        raise ValueError('sealed pre-development training artifacts required')
    a=json.loads((first/'results.json').read_text());b=json.loads((second/'results.json').read_text());compare(a,b)
    for root,result in [(first,a),(second,b)]:
        for name,pin in result['files'].items():
            if digest(root/name)!=pin:raise ValueError('frozen training array drift')
        for name,pin in result['code_pins'].items():
            if digest(Path(__file__).with_name(name))!=pin:raise ValueError('frozen training implementation drift')
    return a


def capture(first,second,destination):
    sealed(first,second)
    if destination.exists() or destination.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git development source required')
    destination.mkdir(parents=True)
    url=f'https://raw.githubusercontent.com/HuiyuanXie/tiage/{COMMIT}/{PATH}'
    with urllib.request.urlopen(url,timeout=30) as response:raw=response.read(SIZE+1)
    pin=verify(raw,BLOB,SIZE)
    with (destination/'anno_dev.json').open('xb') as f:f.write(raw)
    with (destination/'capture.json').open('x') as f:json.dump(
        {'schema_version':1,'commit':COMMIT,'path':PATH,'git_blob':BLOB,'bytes':SIZE,'sha256':pin,
         'calibration_sha256':FIRST,'repeat_sha256':REPEAT,'test_downloaded':False},f,indent=2)
    print(json.dumps({'development_source_sha256':pin,'sealed_calibration_sha256':FIRST}),flush=True)


def experiment(first,second,training,development,snapshot,destination):
    started=time.perf_counter();cpu=time.process_time();reference=sealed(first,second)
    _,training_pin,train=load(training)
    if training_pin!=reference['source_sha256']:raise ValueError('original training source drift')
    metadata=json.loads((development/'capture.json').read_text());raw=(development/'anno_dev.json').read_bytes()
    source_pin=verify(raw,BLOB,SIZE)
    if (metadata!={'schema_version':1,'commit':COMMIT,'path':PATH,'git_blob':BLOB,'bytes':SIZE,'sha256':source_pin,
                   'calibration_sha256':FIRST,'repeat_sha256':REPEAT,'test_downloaded':False}
            or {p.name for p in development.iterdir()}!={'capture.json','anno_dev.json'}):
        raise ValueError('sealed development-only capture required')
    dialogues=parse(raw)
    if len(dialogues)!=100:raise ValueError('complete original development cohort required')
    if digest(snapshot/'onnx/model_quantized.onnx')!=MODEL or digest(snapshot/'tokenizer.json')!=TOKENIZER:
        raise ValueError('pinned explicitly installed assets required')
    unique=list(dict.fromkeys(t.text for d in dialogues for t in d.turns))
    if len(unique)>5000:raise ValueError('complete development unique-input budget exceeded')
    tokenizer=Tokenizer.from_file(str(snapshot/'tokenizer.json'));tokenizer.no_truncation();tokenizer.no_padding()
    tokens=[len(tokenizer.encode(text).ids) for text in unique]
    if any(not 2<=n<=256 or len(text.encode())>65536 for n,text in zip(tokens,unique)):
        raise ValueError('unsupported complete source; no truncation or evaluation repair')
    preflight_ms=(time.perf_counter()-started)*1000
    begin=time.perf_counter();encoder=Encoder(snapshot);init_ms=(time.perf_counter()-begin)*1000
    if encoder.statistics.model_sha256!=MODEL or encoder.statistics.tokenizer_sha256!=TOKENIZER:
        raise ValueError('model assets drift during initialization')
    lookup={};clears=0;begin=time.perf_counter()
    for text in unique:
        if len(encoder.cache)==1024:encoder.cache.clear();clears+=1
        lookup[text]=encoder.encode(text)
    encode_ms=(time.perf_counter()-begin)*1000
    training_sequences={tuple(t.text for t in d.turns) for d in train};training_texts={t.text for d in train for t in d.turns}
    duplicates=[d.id for d in dialogues if tuple(t.text for t in d.turns) in training_sequences]
    rows=[];scores=[];boundaries=[];abstentions=[];offset=0
    for dialogue in dialogues:
        vectors=np.stack([lookup[t.text] for t in dialogue.turns]);local,missing=score_turns(vectors)
        rows.extend(vectors);scores.extend(local);abstentions.extend({'dialogue':dialogue.id,**record} for record in missing)
        for t in dialogue.turns[1:]:boundaries.append({'id':f'dev:{dialogue.id}:{t.ordinal}','dialogue':dialogue.id,'ordinal':t.ordinal,
             'previous_vector_row':offset+t.ordinal-1,'current_vector_row':offset+t.ordinal,'label':t.shift})
        offset+=len(vectors)
    vectors=np.stack(rows);score_matrix=np.stack(scores);truth=np.array([b['label']==Shift.CHANGE for b in boundaries])
    groups=np.array([b['dialogue'] for b in boundaries]);supported=np.isfinite(score_matrix).all(axis=1)
    if not supported.any():raise ValueError('empty matched development support')
    files={}
    for name,array in [('turn-vectors.npy',vectors),('boundary-scores.npy',score_matrix),('common-support.npy',supported)]:
        path=destination/name;np.save(path,array,allow_pickle=False);files[name]=digest(path)
    results={}
    masks={'matched':supported,'dialogue_disjoint':supported&~np.isin(groups,duplicates)}
    for column,method in enumerate(Method):
        threshold=reference['results'][method.value]['winner']['threshold'];slices={}
        for scope,mask in masks.items():
            gold=truth[mask];values=score_matrix[mask,column];prediction=values<threshold
            records=[{'id':d.id,**confusion(truth[mask&(groups==d.id)],score_matrix[mask&(groups==d.id),column]<threshold)}
                     for d in dialogues if (mask&(groups==d.id)).any()]
            slices[scope]={'counts':confusion(gold,prediction),'per_dialogue':records,
                          'average_precision':float(average_precision_score(gold,-values)) if gold.any() and (~gold).any() else None,
                          'roc_auc':float(roc_auc_score(gold,-values)) if gold.any() and (~gold).any() else None,
                          'bootstrap':bootstrap(records) if records else None}
            slices[scope]['paired_all_change_bootstrap']=paired_baseline(records) if records else None
        results[method.value]={'threshold':threshold,'slices':slices}
    baselines={scope:{'no_change':confusion(truth[mask],np.zeros(int(mask.sum()),dtype=bool)),
                      'all_change':confusion(truth[mask],np.ones(int(mask.sum()),dtype=bool))} for scope,mask in masks.items()}
    statistics=asdict(encoder.statistics);inference_ms=statistics.pop('inference_ms');tokenization_ms=statistics.pop('tokenization_ms')
    return {'schema_version':1,'scope':'sealed-training-cutoff-development-evaluation-not-topic-selection',
            'python':platform.python_version(),'calibration_sha256':FIRST,'calibration_repeat_sha256':REPEAT,
            'source_sha256':source_pin,'capture_sha256':digest(development/'capture.json'),'training_source_sha256':training_pin,
            'model':statistics,'files':files,'dialogue_ids':[d.id for d in dialogues],'boundaries':boundaries,
            'abstentions':abstentions,'frozen_global_winner':reference['winner']['method'],'results':results,'baselines':baselines,
            'support':{'original':len(truth),'matched':int(supported.sum()),'original_turns':len(vectors),'unique_model_inputs':len(unique),
                       'cache_clears':clears,'maximum_preflight_tokens':max(tokens)},
            'overlap':{'exact_training_dialogue_ids':duplicates,'training_shared_unique_texts':len(set(unique)&training_texts),
                       'training_shared_turn_occurrences':sum(t.text in training_texts for d in dialogues for t in d.turns)},
            'test_downloaded_or_scored':False,
            'code_pins':{name:digest(Path(__file__).with_name(name)) for name in
                         ['tiage_development.py','tiage_calibration.py','boundary_scoring.py','tiage.py','local_embeddings.py','uv.lock']},
            'timings':{'preflight_ms':preflight_ms,'model_init_ms':init_ms,'encode_wall_ms':encode_ms,
                       'inference_ms':inference_ms,'tokenization_ms':tokenization_ms,'total_ms':(time.perf_counter()-started)*1000,
                       'process_cpu_ms':(time.process_time()-cpu)*1000,'peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}}


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for key in ['calibration','calibration-repeat']:p.add_argument('--'+key,type=Path,required=True)
    p.add_argument('--capture-development',type=Path)
    for key in ['training-source','development-source','snapshot','output-directory']:p.add_argument('--'+key,type=Path)
    p.add_argument('--compare',type=Path);a=p.parse_args()
    if a.capture_development:
        if any([a.training_source,a.development_source,a.snapshot,a.output_directory,a.compare]):raise ValueError('capture and evaluation are separate phases')
        capture(a.calibration,a.calibration_repeat,a.capture_development);return
    if any(v is None for v in [a.training_source,a.development_source,a.snapshot,a.output_directory]):raise ValueError('complete evaluation inputs required')
    dst=a.output_directory
    if dst.exists() or dst.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git evaluation output required')
    dst.mkdir(parents=True);r=json.loads(json.dumps(experiment(a.calibration,a.calibration_repeat,a.training_source,a.development_source,a.snapshot,dst),allow_nan=False))
    if a.compare:compare(json.loads(a.compare.read_text()),r)
    with (dst/'results.json').open('x') as f:json.dump(r,f,indent=2,allow_nan=False)
    print(json.dumps({'winner':r['frozen_global_winner'],'results':{m:v['slices']['matched']['counts'] for m,v in r['results'].items()},
                      'overlap':r['overlap'],'support':r['support'],'timings':r['timings']}),flush=True)


if __name__=='__main__':main()
