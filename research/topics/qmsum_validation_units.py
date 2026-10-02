"""Held-out original readiness/boundaries only; no queries, fitting or inference."""
import argparse
from collections import Counter
from dataclasses import asdict
import hashlib
import json
from pathlib import Path
import resource
import time
from tokenizers import Tokenizer
from qmsum import load as training
from qmsum_validation_capture import load, check_disjoint
from qmsum_semantic_preflight import TOKENIZER
from marker_inputs import mask, readiness, Readiness
from validation_blocks import boundaries


PINS = ['qmsum_validation_units.py','GUIDANCE_VALIDATION_PROTOCOL.md','qmsum_validation_capture.py',
        'validation_blocks.py','episode_blocks.py','marker_inputs.py','label_noise.py','qmsum.py','uv.lock']


def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()


def experiment(root, assets):
    start = time.perf_counter(); cpu = time.process_time(); pins = {n:digest(Path(__file__).with_name(n)) for n in PINS}
    source = load(root/'topic-qmsum-author-validation-20261002')
    _,_,train = training(root/'topic-qmsum-author-training-20261002'); split = check_disjoint(source,train)
    tokenizer_path = assets/'tokenizer.json'
    if digest(tokenizer_path) != TOKENIZER:raise ValueError('explicit installed frozen tokenizer required; no downloads')
    tokenizer = Tokenizer.from_file(str(tokenizer_path));tokenizer.no_padding();tokenizer.no_truncation()
    rows = []; meetings = []; states = {'raw':Counter(),'masked':Counter()}; offset = 0
    for meeting in source.meetings:
        ready = []; tokens = []; local = []
        for turn in meeting.turns:
            masked,_ = mask(turn.text,offset+turn.ordinal); variants = {}
            for name,text in [('raw',turn.text),('masked',masked)]:
                n = len(tokenizer.encode(text).ids); state = readiness(text,n); states[name][state] += 1
                variants[name] = {'text_sha256':hashlib.sha256(text.encode()).hexdigest(),'tokens':n,'status':state}
            flag = all(v['status'] == Readiness.READY for v in variants.values());ready.append(flag);tokens.append(variants['raw']['tokens'])
            local.append({'meeting':meeting.ordinal,'ordinal':turn.ordinal,'source_row':offset+turn.ordinal,'ready':flag,'variants':variants})
        blocks,owners = boundaries(ready,tokens)
        rows.extend(local);meetings.append({'meeting':meeting.ordinal,'source_offset':offset,'source_count':len(local),
            'blocks':[asdict(b) for b in blocks],'owners':list(owners)})
        offset += len(local)
    if offset != 20123 or len(meetings) != 35 or pins != {n:digest(Path(__file__).with_name(n)) for n in PINS} or digest(tokenizer_path) != TOKENIZER:
        raise ValueError('complete held-out source/tokenizer/code support drift')
    return {'schema_version':1,'scope':'validation_source_only_units_no_features_or_quality',
        'capture_sha256':source.capture_sha256,'source_sha256':source.source_sha256,'protocol_sha256':source.protocol_sha256,
        'tokenizer_sha256':TOKENIZER,'split_check':asdict(split),'code_pins':pins,'rows':rows,'meetings':meetings,
        'support':{'sources':offset,'ready':sum(r['ready'] for r in rows),'blocks':sum(len(m['blocks']) for m in meetings),
            'status_counts':{k:dict(v) for k,v in states.items()}},
        'protocol':{'new_inference':0,'llm_calls':0,'queries_scored':0,'annotations_used':False,'truncation':False,'padding':False},
        'timings':{'after_imports_ms':(time.perf_counter()-start)*1000,'cpu_ms':(time.process_time()-cpu)*1000,
            'rusage_peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}}


def main():
    p = argparse.ArgumentParser(description=__doc__);p.add_argument('--artifacts',type=Path,required=True)
    p.add_argument('--assets',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--compare',type=Path);a = p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    result = experiment(a.artifacts,a.assets)
    if a.compare and {k:v for k,v in json.loads(a.compare.read_text()).items() if k != 'timings'} != {k:v for k,v in result.items() if k != 'timings'}:
        raise ValueError('complete non-timing unit repeat differs')
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps({'support':result['support'],'timings':result['timings']}),flush=True)


if __name__ == '__main__':main()
