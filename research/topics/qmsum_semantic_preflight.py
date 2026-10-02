"""Complete raw/masked source and tokenizer gate; no model inference."""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import resource
import time

from tokenizers import Tokenizer

from external_context import compare
from marker_inputs import mask,readiness
from qmsum import load


MODEL='afdb6f1a0e45b715d0bb9b11772f032c399babd23bfc31fed1c170afc848bdb1'
TOKENIZER='da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0'


def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()


def experiment(a):
    start=time.perf_counter();cpu=time.process_time();capture,source,meetings=load(a.source)
    if digest(a.assets/'onnx/model_quantized.onnx')!=MODEL or digest(a.assets/'tokenizer.json')!=TOKENIZER:raise ValueError('explicit installed asset drift')
    tokenizer=Tokenizer.from_file(str(a.assets/'tokenizer.json'));tokenizer.no_truncation();tokenizer.no_padding()
    rows=[];ledger=[];summary={'raw':Counter(),'masked':Counter()};unique={'raw':set(),'masked':set()};common=0
    for meeting in meetings:
        for turn in meeting.turns:
            i=len(rows);masked,markers=mask(turn.text,i);ledger.extend(markers);entry={'meeting':meeting.ordinal,'ordinal':turn.ordinal,'source_row':i,'variants':{}}
            for policy,text in [('raw',turn.text),('masked',masked)]:
                encoding=tokenizer.encode(text);tokens=len(encoding.ids);state=readiness(text,tokens)
                pin=hashlib.sha256(text.encode()).hexdigest()
                entry['variants'][policy]={'text_sha256':pin,'characters':len(text),'utf8_bytes':len(text.encode()),'tokens':tokens,'status':state}
                summary[policy][state]+=1;unique[policy].add(pin)
            common+=int(all(v['status']=='ready' for v in entry['variants'].values()));rows.append(entry)
    if len(rows)!=88336:raise ValueError('incomplete original source support')
    return {'schema_version':1,'scope':'complete_source_tokenizer_gate_no_inference','source_sha256':source,'capture_sha256':capture,
            'model_sha256':MODEL,'tokenizer_sha256':TOKENIZER,'protocol':{'mask':'exact_braced_ASCII_markers_to_equal_length_spaces',
            'source_coordinate_mapping':'identity_except_marker_content','tokenization':'singleton_unpadded_untruncated_special_tokens_included',
            'maximum_model_tokens':512,'no_source_truncation':True,'new_inference_calls':0,'held_out_sources_read':False},
            'rows':rows,'source_turns':len(rows),'common_ready_rows':common,'status_counts':{k:dict(v) for k,v in summary.items()},
            'exact_unique_input_hashes':{k:len(v) for k,v in unique.items()},'marker_ledger':ledger,
            'marker_counts':dict(Counter(m['term'] for m in ledger)),
            'code_pins':{name:digest(Path(__file__).with_name(name)) for name in
                         ['qmsum_semantic_preflight.py','marker_inputs.py','label_noise.py','qmsum.py','uv.lock']},
            'timings':{'after_imports_ms':(time.perf_counter()-start)*1000,'cpu_ms':(time.process_time()-cpu)*1000,
                       'peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}}


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for key in ['source','assets','output']:p.add_argument('--'+key,type=Path,required=True)
    p.add_argument('--compare',type=Path);a=p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    r=experiment(a)
    if a.compare:compare(json.loads(a.compare.read_text()),json.loads(json.dumps(r,allow_nan=False)))
    with a.output.open('x') as f:json.dump(r,f,indent=2,allow_nan=False)
    print(json.dumps({k:r[k] for k in ['status_counts','common_ready_rows','exact_unique_input_hashes','marker_counts','timings']}),flush=True)


if __name__=='__main__':main()
