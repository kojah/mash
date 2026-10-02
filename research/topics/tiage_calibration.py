"""Frozen training-only online boundary calibration; development/test remain absent."""
import argparse
from dataclasses import asdict
import hashlib
import json
from pathlib import Path
import platform
import resource
import time

import numpy as np
from sklearn.metrics import average_precision_score,roc_auc_score
from tokenizers import Tokenizer

from boundary_scoring import Method,calibrate,choose,confusion,score_turns
from external_context import compare
from local_embeddings import Encoder
from tiage import Shift,load


PREFLIGHT='71762aa571039f30fc6188559ef1221c63f44e52b932ba1528c191615ee4e4a5'
MODEL='afdb6f1a0e45b715d0bb9b11772f032c399babd23bfc31fed1c170afc848bdb1'
TOKENIZER='da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0'


def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()


def bootstrap(records,seed=20261002):
    # Dialogue-clustered resampling; training optimism remains, not a test CI.
    rng=np.random.default_rng(seed);matrix=np.array([[r[k] for k in ['tp','fp','fn','tn']] for r in records])
    totals=matrix[rng.integers(0,len(matrix),size=(1000,len(matrix)))].sum(axis=1)
    denominator=2*totals[:,0]+totals[:,1]+totals[:,2]
    f1=np.divide(2*totals[:,0],denominator,out=np.zeros(len(totals),dtype=float),where=denominator>0)
    return {'draws':1000,'seed':seed,'f1_percentiles':np.percentile(f1,[2.5,50,97.5]).tolist(),
            'limitation':'training-selected cutoff; percentile range is not held-out significance'}


def experiment(source,preflight,snapshot,destination):
    started=time.perf_counter();cpu=time.process_time()
    if digest(preflight)!=PREFLIGHT:raise ValueError('pinned final complete source preflight required')
    previous=json.loads(preflight.read_text());capture_pin,source_pin,dialogues=load(source)
    if (capture_pin!=previous['capture_sha256'] or source_pin!=previous['source_sha256']
            or [d.id for d in dialogues]!=[d['id'] for d in previous['ledger']]):raise ValueError('original source identity drift')
    if digest(snapshot/'onnx/model_quantized.onnx')!=MODEL or digest(snapshot/'tokenizer.json')!=TOKENIZER:
        raise ValueError('explicit pinned installed assets required; no downloads')
    unique=list(dict.fromkeys(t.text for d in dialogues for t in d.turns))
    if len(unique)>5000:raise ValueError('complete unique-input budget exceeded; no truncation')
    tokenizer=Tokenizer.from_file(str(snapshot/'tokenizer.json'));tokenizer.no_truncation();tokenizer.no_padding()
    tokens=[len(tokenizer.encode(text).ids) for text in unique]
    if any(not 2<=n<=256 or len(text.encode())>65536 for n,text in zip(tokens,unique)):
        raise ValueError('unsupported complete source; freeze a new window protocol before inference')
    preflight_ms=(time.perf_counter()-started)*1000
    begin=time.perf_counter();encoder=Encoder(snapshot);model_init_ms=(time.perf_counter()-begin)*1000
    if encoder.statistics.model_sha256!=MODEL or encoder.statistics.tokenizer_sha256!=TOKENIZER:
        raise ValueError('model asset drift during initialization')
    lookup={};clears=0
    begin=time.perf_counter()
    for i,text in enumerate(unique):
        if len(encoder.cache)==1024:encoder.cache.clear();clears+=1
        lookup[text]=encoder.encode(text)
        if (i+1)%1024==0:print(json.dumps({'encoded_unique':i+1,'total_unique':len(unique)}),flush=True)
    encode_wall_ms=(time.perf_counter()-begin)*1000
    rows=[];boundaries=[];scores=[];abstentions=[];offset=0;labels=[]
    for dialogue in dialogues:
        vectors=np.stack([lookup[t.text] for t in dialogue.turns]);local,missing=score_turns(vectors)
        rows.extend(vectors);scores.extend(local)
        abstentions.extend({'dialogue':dialogue.id,**record} for record in missing)
        for t in dialogue.turns[1:]:
            boundaries.append({'id':f'train:{dialogue.id}:{t.ordinal}','dialogue':dialogue.id,
                               'ordinal':t.ordinal,'previous_vector_row':offset+t.ordinal-1,
                               'current_vector_row':offset+t.ordinal,'label':t.shift})
            labels.append(t.shift==Shift.CHANGE)
        offset+=len(vectors)
    vectors=np.stack(rows);score_matrix=np.stack(scores);truth=np.array(labels,dtype=bool)
    if len(vectors)!=previous['turns'] or len(truth)!=previous['scorable_original_boundaries']:
        raise ValueError('complete original turn/boundary support required')
    supported=np.isfinite(score_matrix).all(axis=1)
    if not supported.any():raise ValueError('empty common supported boundary set')
    begin=time.perf_counter();results={}
    for column,method in enumerate(Method):
        values=score_matrix[supported,column];gold=truth[supported];result=calibrate(values,gold)
        predicted=values<result['winner']['threshold'];by_dialogue=[]
        groups=np.array([b['dialogue'] for b in boundaries])[supported]
        for dialogue in dialogues:
            mask=groups==dialogue.id;by_dialogue.append({'id':dialogue.id,**confusion(gold[mask],predicted[mask])})
        result.update(average_precision=float(average_precision_score(gold,-values)),
                      roc_auc=float(roc_auc_score(gold,-values)),per_dialogue=by_dialogue,
                      bootstrap=bootstrap(by_dialogue))
        results[method.value]=result
    winner=choose(results);calibration_ms=(time.perf_counter()-begin)*1000
    files={}
    for name,array in [('turn-vectors.npy',vectors),('boundary-scores.npy',score_matrix),('common-support.npy',supported)]:
        path=destination/name;np.save(path,array,allow_pickle=False);files[name]=digest(path)
    baselines={scope:{'no_change':confusion(gold,np.zeros(len(gold),dtype=bool)),
                      'all_change':confusion(gold,np.ones(len(gold),dtype=bool))}
               for scope,gold in [('original',truth),('matched',truth[supported])]}
    model_statistics=asdict(encoder.statistics)
    inference_ms=model_statistics.pop('inference_ms');tokenization_ms=model_statistics.pop('tokenization_ms')
    return {'schema_version':1,'scope':'training-only-boundary-calibration-not-topic-selection',
            'python':platform.python_version(),'source_capture_sha256':capture_pin,'source_sha256':source_pin,
            'preflight_sha256':PREFLIGHT,'model':model_statistics,'files':files,
            'dialogue_ids':[d.id for d in dialogues],'boundaries':boundaries,'abstentions':abstentions,
            'support':{'original':len(truth),'matched':int(supported.sum()),'unique_model_inputs':len(unique),
                       'original_turns':len(vectors),'exact_text_reuses':len(vectors)-len(unique),
                       'cache_clears':clears,'maximum_preflight_tokens':max(tokens)},
            'parameters':{'prediction':'cosine < threshold','context':'previous at most two normalized turn directions',
                          'cancellation_norm':1e-12,'objective':'maximum exact rational training micro F1',
                          'ties':['fewer predicted changes','lexical method ID','smaller threshold'],
                          'candidates':'all distinct training scores plus nextafter(max,+inf)',
                          'inference':'singleton complete text; normalized round-away int8 direction',
                          'maximum_unique_inputs':5000,'maximum_tokens':256},
            'results':results,'winner':{'method':winner,**results[winner]['winner']},'baselines':baselines,
            'development_test_downloaded_or_scored':False,
            'code_pins':{name:digest(Path(__file__).with_name(name)) for name in
                         ['tiage_calibration.py','boundary_scoring.py','tiage.py','tiage_capture.py','local_embeddings.py','uv.lock']},
            'timings':{'preflight_ms':preflight_ms,'model_init_ms':model_init_ms,'encode_wall_ms':encode_wall_ms,
                       'inference_ms':inference_ms,'tokenization_ms':tokenization_ms,
                       'calibration_bootstrap_ms':calibration_ms,'total_ms':(time.perf_counter()-started)*1000,
                       'process_cpu_ms':(time.process_time()-cpu)*1000,
                       'peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}}


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for key in ['source','preflight','snapshot','output-directory']:p.add_argument('--'+key,type=Path,required=True)
    p.add_argument('--compare',type=Path);args=p.parse_args();dst=args.output_directory
    if dst.exists() or dst.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    dst.mkdir(parents=True);result=json.loads(json.dumps(experiment(args.source,args.preflight,args.snapshot,dst),allow_nan=False))
    if args.compare:compare(json.loads(args.compare.read_text()),result)
    with (dst/'results.json').open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps({'winner':result['winner'],'support':result['support'],'timings':result['timings']}),flush=True)


if __name__=='__main__':main()
