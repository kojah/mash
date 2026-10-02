"""Complete-window local embeddings for admitted text; preserve excluded records."""
import argparse
from dataclasses import asdict
import hashlib
import json
from pathlib import Path
import resource
import time
import numpy as np
from embedding_windows import Field,windows
from local_embeddings import Encoder,quantized_direction
from window_inference import infer
from topic_stream import Source
from topic_stream_demo import run,pair_members
from topic_units import units,route


BASELINE='367d53bff53eb4186123992b3fbad84fd8ea900bce6829d701dfe58783125cc7'
MODEL='afdb6f1a0e45b715d0bb9b11772f032c399babd23bfc31fed1c170afc848bdb1'
TOKENIZER='da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0'
PINS=['topic_window_control.py','embedding_windows.py','local_embeddings.py','window_inference.py',
      'topic_units.py','topic_stream.py','topic_refresh.py','topic_jobs.py','topic_stream_demo.py',
      'data.py','episode_blocks.py','block_hierarchy.py','captured_hierarchy.py','subject_labels.py','label_noise.py','uv.lock']


def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()


def stable_json(result):
    value=dict(result);value.pop('runtime')
    return json.dumps(value,sort_keys=True,allow_nan=False,separators=(',',':'))


def combine(raw,weights):
    values=np.asarray(raw,dtype=np.float32);weights=np.asarray(weights,dtype=np.float32)
    if values.ndim!=2 or values.shape[1]!=384 or len(values)!=len(weights) or not len(values):
        raise ValueError('aligned window vectors and payload weights required')
    if not np.isfinite(values).all() or not np.isfinite(weights).all() or (weights<=0).any():
        raise ValueError('finite positive payload weights required')
    if len(values)==1:return quantized_direction(values[0])
    return quantized_direction((values*(weights/weights.sum())[:,None]).sum(axis=0,dtype=np.float32))


def experiment(baseline,assets):
    if digest(baseline)!=BASELINE:raise ValueError('frozen original prototype baseline required')
    if digest(assets/'onnx/model_quantized.onnx')!=MODEL or digest(assets/'tokenizer.json')!=TOKENIZER:
        raise ValueError('pinned installed assets required; no downloads')
    pins={name:digest(Path(__file__).with_name(name)) for name in PINS}
    report=json.loads(baseline.read_text());records=report['baseline_checkpoint']['records']
    source_order={p['id']:(p['thread'],p['ordinal'],p['original_message']) for p in report['source_provenance']}
    start=time.perf_counter();cpu=time.process_time();encoder=Encoder(assets)
    variants={name:[] for name in ['first_window','complete_windows']};coverage=[];calls=0;inference_ms=0.
    # Original round-robin message arrival, before grouping, matches the baseline.
    by_thread={}
    for row in sorted(records,key=lambda r:source_order[r['id']]):by_thread.setdefault(row['thread'],[]).append(row)
    rows=[by_thread[t][i] for i in range(max(map(len,by_thread.values()))) for t in sorted(by_thread) if i<len(by_thread[t])]
    for row in rows:
        text=row['text'];parts=windows(encoder.tokenizer,row['id'],Field.BODY,text)
        if bool(parts)!=(row['vector'] is not None):raise ValueError('admission and stored coverage differ')
        pooled=[]
        for part in parts:
            before=time.perf_counter();raw,_=infer(encoder.session,[part]);inference_ms+=(time.perf_counter()-before)*1000
            pooled.append(raw[0]);calls+=1
        first=combine(pooled[:1],[p.payload_tokens for p in parts[:1]]) if parts else None
        complete=combine(pooled,[p.payload_tokens for p in parts]) if parts else None
        for name,vector in [('first_window',first),('complete_windows',complete)]:
            variants[name].append(Source(row['id'],row['thread'],row['provider'],text,vector))
        coverage.append({'source':row['id'],'status':'admitted' if parts else 'excluded_empty_projection',
                         'source_text_sha256':hashlib.sha256(text.encode()).hexdigest(),
                         'source_bytes':len(text.encode()),'source_characters':len(text),
                         'windows':[asdict(part) for part in parts],
                         'first_window_bytes':parts[0].byte_end if parts else 0,
                         'complete_bytes':sum(p.byte_end-p.byte_start for p in parts)})
        if calls and len(coverage)%20==0:
            print(json.dumps({'processed_sources':len(coverage),'window_calls':calls}),flush=True)
    encoding_ms=(time.perf_counter()-start)*1000;arms={};runtime={}
    for name,sources in variants.items():
        begin=time.perf_counter();_,forward,fr=run(sources);_,reverse,rr=run(list(reversed(sources)))
        native=sorted(sources,key=lambda s:source_order[s.id]);groups=units(native)
        pooled=route(groups);pooled_reverse=route(list(reversed(groups)))
        def order(left,right):
            a=pair_members(left);b=pair_members(right)
            return len(a&b)/len(a|b) if a|b else None
        arms[name]={'vectors':{s.id:None if s.vector is None else s.vector.tolist() for s in sources},
                    'raw_forward':forward,'raw_reverse':reverse,'unit_forward':pooled,'unit_reverse':pooled_reverse,
                    'units':[[s.id for s in group] for group in groups],
                    'order':{'raw_pair_jaccard':order(forward,reverse),'unit_pair_jaccard':order(pooled,pooled_reverse)}}
        runtime[name]={'wall_ms':(time.perf_counter()-begin)*1000,'forward':fr,'reverse':rr}
    if digest(baseline)!=BASELINE or pins!={name:digest(Path(__file__).with_name(name)) for name in PINS}:
        raise ValueError('source or code drift during control')
    return {'schema_version':1,'baseline_sha256':BASELINE,'model_sha256':MODEL,'tokenizer_sha256':TOKENIZER,
            'code_pins':pins,'model_runtime':{'onnxruntime':encoder.statistics.onnxruntime_version,
                                            'tokenizers':encoder.statistics.tokenizers_version,'cpu_threads':1,'batch_size':1},
            'policy':{'full_text':'all admitted original represented text, contiguous whole-word windows, no truncation',
                      'window_tokens':256,'pooling':'attended float32 means then payload-weighted raw means; quantize once',
                      'excluded_empty_projection':'retain source, no embedding or borrowed content','llm_calls':0},
            'coverage':coverage,'inference_calls':calls,'arms':arms,'production_selected':False,'semantic_quality_verified':False,
            'runtime':{'encoding_including_model_setup_ms':encoding_ms,'inference_ms':inference_ms,'arms':runtime,
                       'total_ms':(time.perf_counter()-start)*1000,'cpu_ms':(time.process_time()-cpu)*1000,
                       'rusage_peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['baseline','assets','output']:parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--compare',type=Path);args=parser.parse_args()
    result=experiment(args.baseline,args.assets)
    if args.compare:
        if stable_json(json.loads(args.compare.read_text()))!=stable_json(result):
            raise ValueError('non-runtime complete-window control differs')
    with args.output.open('x') as file:json.dump(result,file,sort_keys=True,allow_nan=False)
    args.output.chmod(0o600)
    print(json.dumps({'calls':result['inference_calls'],'admitted':sum(x['status']=='admitted' for x in result['coverage']),
                      'arms':{name:{'raw':arm['raw_forward']['diagnostics'],'units':arm['unit_forward']['diagnostics'],'order':arm['order']}
                              for name,arm in result['arms'].items()},'total_ms':result['runtime']['total_ms']}))
