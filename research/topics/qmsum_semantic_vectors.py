"""Frozen singleton local semantic representations on complete raw/masked source."""
import argparse
from dataclasses import asdict
import hashlib
import json
from pathlib import Path
import resource
import time

import numpy as np

from external_context import compare
from local_embeddings import Encoder
from marker_inputs import mask
from qmsum import load
from qmsum_semantic_preflight import MODEL,TOKENIZER


PREFLIGHT='be77327996bc0a517cadad8d12074470ad6fdecedb17f40b444c18bddaeb0620'


def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()


def experiment(a):
    start=time.perf_counter();cpu=time.process_time()
    if digest(a.preflight)!=PREFLIGHT:raise ValueError('frozen complete original preflight required')
    preflight=json.loads(a.preflight.read_text());capture,source,meetings=load(a.source)
    if (capture,source)!=(preflight['capture_sha256'],preflight['source_sha256']):raise ValueError('original source drift')
    for name,pin in preflight['code_pins'].items():
        if digest(Path(__file__).with_name(name))!=pin:raise ValueError('frozen preflight implementation drift')
    if digest(a.assets/'onnx/model_quantized.onnx')!=MODEL or digest(a.assets/'tokenizer.json')!=TOKENIZER:raise ValueError('installed asset drift')
    encoder=Encoder(a.assets)
    if encoder.statistics.model_sha256!=MODEL or encoder.statistics.tokenizer_sha256!=TOKENIZER:raise ValueError('asset drift during initialization')
    arrays={name:np.lib.format.open_memmap(a.output_directory/f'{name}-vectors.npy',mode='w+',dtype=np.float64,shape=(88336,384))
            for name in ['raw','masked']}
    for value in arrays.values():value[:]=0.
    setup_ms=(time.perf_counter()-start)*1000;begin=time.perf_counter();seen={};source_row=0;clears=0;reused=0;records=[]
    for meeting in meetings:
        for turn in meeting.turns:
            expected=preflight['rows'][source_row]
            if (expected['meeting'],expected['ordinal'],expected['source_row'])!=(meeting.ordinal,turn.ordinal,source_row):raise ValueError('preflight source identity drift')
            masked,_=mask(turn.text,source_row);record={'meeting':meeting.ordinal,'ordinal':turn.ordinal,'source_row':source_row,'variants':{}}
            for name,text in [('raw',turn.text),('masked',masked)]:
                entry=expected['variants'][name]
                if hashlib.sha256(text.encode()).hexdigest()!=entry['text_sha256']:raise ValueError('source model input drift')
                origin=None
                if entry['status']=='ready':
                    if text in seen:
                        origin=seen[text];arrays[name][source_row]=arrays[origin[0]][origin[1]];reused+=1
                    else:
                        if len(encoder.cache)==1024:encoder.cache.clear();clears+=1
                        vector=encoder.encode(text)
                        if vector.shape!=(384,) or not np.isfinite(vector).all() or not np.isclose(np.linalg.norm(vector),1,rtol=0,atol=1e-12):raise ValueError('invalid frozen local direction')
                        arrays[name][source_row]=vector;origin=(name,source_row);seen[text]=origin
                record['variants'][name]={'status':entry['status'],'text_sha256':entry['text_sha256'],
                                         'tokens':entry['tokens'],'vector_origin':list(origin) if origin else None}
            records.append(record);source_row+=1
        if meeting.ordinal%10==0:
            elapsed=time.perf_counter()-begin
            print(json.dumps({'processed_meetings':meeting.ordinal+1,'source_turns':source_row,'unique_inference_calls':encoder.statistics.calls,
                              'elapsed_s':elapsed,'source_eta_s':elapsed*(88336-source_row)/source_row}),flush=True)
    if source_row!=88336 or encoder.statistics.calls!=len(seen):raise ValueError('incomplete source or unique-call accounting')
    inference_ms=(time.perf_counter()-begin)*1000;begin=time.perf_counter();files={}
    for name,array in arrays.items():
        array.flush();path=a.output_directory/f'{name}-vectors.npy';files[path.name]={'sha256':digest(path),'shape':list(array.shape),'dtype':str(array.dtype)}
    result={'schema_version':1,'scope':'frozen_complete_semantic_representations_not_quality_or_method_selection',
            'source_sha256':source,'capture_sha256':capture,'preflight_sha256':PREFLIGHT,'model_sha256':MODEL,'tokenizer_sha256':TOKENIZER,
            'protocol':{'policies':['raw','masked'],'model':'installed_quantized_all_MiniLM_L6_v2','cpu_threads':1,'batch_size':1,
                        'padding':False,'truncation':False,'pooling':'all_attended_tokens_mean_float32',
                        'direction':'scale127_by_max_abs_round_ties_away_int8_then_float64_l2',
                        'unsupported_vector_storage':'zero_row_with_explicit_status_not_searchable',
                        'reuse':'exact_input_text_shared_across_policies','encoder_cache_maximum':1024,'held_out_sources_read':False},
            'files':files,'rows':records,'status_counts':preflight['status_counts'],'common_ready_rows':preflight['common_ready_rows'],
            'source_turns':source_row,'unique_inference_calls':encoder.statistics.calls,'reused_saved_vectors':reused,
            'cache_clears':clears,'encoder_statistics':{k:v for k,v in asdict(encoder.statistics).items() if k not in ['inference_ms','tokenization_ms']},
            'code_pins':{name:digest(Path(__file__).with_name(name)) for name in
                         ['qmsum_semantic_vectors.py','local_embeddings.py','data.py','marker_inputs.py','label_noise.py','qmsum.py','uv.lock']}}
    result['timings']={'setup_ms':setup_ms,'encoding_stage_ms':inference_ms,'flush_hash_ms':(time.perf_counter()-begin)*1000,
                       'total_after_imports_ms':(time.perf_counter()-start)*1000,'cpu_ms':(time.process_time()-cpu)*1000,
                       'model_inference_ms':encoder.statistics.inference_ms,'tokenization_ms':encoder.statistics.tokenization_ms,
                       'peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}
    return result


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for key in ['source','assets','preflight','output-directory']:p.add_argument('--'+key,type=Path,required=True)
    p.add_argument('--compare',type=Path);a=p.parse_args()
    if a.output_directory.exists() or a.output_directory.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    a.output_directory.mkdir(parents=True);r=experiment(a)
    if a.compare:compare(json.loads(a.compare.read_text()),json.loads(json.dumps(r,allow_nan=False)))
    with (a.output_directory/'results.json').open('x') as f:json.dump(r,f,indent=2,allow_nan=False)
    print(json.dumps({k:r[k] for k in ['status_counts','common_ready_rows','unique_inference_calls','reused_saved_vectors','timings']}),flush=True)


if __name__=='__main__':main()
