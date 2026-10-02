"""Pre-outcome quantized batching probe on hash-selected window-length strata."""
import argparse
from dataclasses import asdict
import hashlib
import json
from pathlib import Path
import time

import numpy as np

from cqadup_source import load
from cqadup_window_preflight import CAPTURE, MODEL, TOKENIZER
from embedding_windows import Field, Window
from external_context import compare
from local_embeddings import Encoder
from stackoverflow_source import encoded
from window_inference import infer


def probe(root, preflight, preflight_pin, snapshot):
    started = time.perf_counter()
    raw = preflight.read_bytes()
    if hashlib.sha256(raw).hexdigest() != preflight_pin:
        raise ValueError('explicit pinned preflight bytes required')
    capture = json.loads(raw)
    if (capture['model_sha256'] != MODEL or capture['tokenizer_sha256'] != TOKENIZER
            or capture['retrieval_outcomes'] is not None or capture['encoder_calls'] != 0):
        raise ValueError('pinned no-outcome preflight assets required')
    bins = [(2,32), (33,64), (65,128), (129,256)]
    selected = []
    corpus = [w for field in ('title','body') for r in capture['ledgers'][field] for w in r['windows']]
    for low, high in bins:
        eligible = [w for w in corpus if low <= len(w['ids']) <= high]
        eligible.sort(key=lambda w: hashlib.sha256(encoded(['mash-batch-window-v1-20261002', w])).hexdigest())
        if len(eligible) < 4:
            raise ValueError('complete four-per-length-stratum probe required')
        selected.extend(eligible[:4])
    units = [Window(**(w | {'field': Field(w['field']), 'ids': tuple(w['ids']), 'types': tuple(w['types'])})) for w in selected]
    documents, _, _, _ = load(root, CAPTURE)
    encoder = Encoder(snapshot)
    if encoder.statistics.model_sha256 != MODEL or encoder.statistics.tokenizer_sha256 != TOKENIZER:
        raise ValueError('installed asset drift')
    if encoder.tokenizer.token_to_id('[PAD]') != 0:
        raise ValueError('pinned padding identity required')
    timings, singles, pooled_singles = {}, [], []
    begin = time.perf_counter()
    for unit in units:
        text = getattr(documents[unit.source], 'title' if unit.field == Field.TITLE else 'text')[unit.start:unit.end]
        if hashlib.sha256(text.encode()).hexdigest() != unit.text_sha256 or tuple(encoder.tokenizer.encode(text).ids) != unit.ids:
            raise ValueError('complete source/window input drift')
        pooled, vector = infer(encoder.session, [unit])
        # Existing Python singleton executable reference must match exactly.
        if not np.array_equal(vector[0], encoder.encode(text)):
            raise ValueError('window singleton differs from executable reference')
        pooled_singles.append(pooled[0]); singles.append(vector[0])
    timings['singleton_and_reference_ms'] = (time.perf_counter() - begin)*1000
    singles, pooled_singles = np.stack(singles), np.stack(pooled_singles)
    begin = time.perf_counter()
    pooled_batch, batch = infer(encoder.session, units)
    timings['mixed_batch16_ms'] = (time.perf_counter() - begin)*1000
    statistics = asdict(encoder.statistics)
    for name in ('inference_ms','tokenization_ms'):
        timings[name] = statistics.pop(name)
    records = []
    for i, unit in enumerate(units):
        records.append({'unit': selected[i], 'singleton_pooled': pooled_singles[i].tolist(),
                        'batch_pooled': pooled_batch[i].tolist(), 'singleton': singles[i].tolist(),
                        'batch': batch[i].tolist(),
                        'cosine': float(np.dot(singles[i], batch[i])),
                        'max_component_drift': float(np.max(np.abs(singles[i]-batch[i]))),
                        'pooled_max_absolute_drift': float(np.max(np.abs(pooled_singles[i]-pooled_batch[i])))})
    return {'version': 1, 'scope': 'length-stratified-batch-fidelity-no-retrieval-outcomes',
            'preflight_sha256': hashlib.sha256(raw).hexdigest(), 'model_sha256': MODEL, 'tokenizer_sha256': TOKENIZER,
            'selection': 'four-hash-selected-per-2-32/33-64/65-128/129-256-corpus-window-stratum',
            'singleton_reference_exact': True, 'encoder_statistics': statistics,
            'model_runs': 17 + statistics['calls'],
            'encoded_window_occurrences': 32 + statistics['calls'],
            'batch_all_vectors_exact': bool(np.array_equal(singles,batch)),
            'minimum_batch_cosine': float(min(r['cosine'] for r in records)),
            'records': records, 'retrieval_outcomes': None,
            'code_pins': {n: hashlib.sha256(Path(__file__).with_name(n).read_bytes()).hexdigest() for n in
                          ('cqadup_batch_probe.py', 'window_inference.py', 'local_embeddings.py',
                           'cqadup_source.py', 'embedding_windows.py', 'uv.lock')},
            'timings': timings | {'total_ms': (time.perf_counter()-started)*1000}}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--root', type=Path, required=True)
    p.add_argument('--preflight', type=Path, required=True)
    p.add_argument('--preflight-sha256', required=True)
    p.add_argument('--snapshot', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--compare', type=Path)
    args = p.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git probe output required')
    result = probe(args.root, args.preflight, args.preflight_sha256, args.snapshot)
    if args.compare:
        compare(json.loads(args.compare.read_text()), result)
    with args.output.open('x') as file:
        json.dump(result, file, sort_keys=True, allow_nan=False)
    print(json.dumps({k: result[k] for k in ('scope','batch_all_vectors_exact','minimum_batch_cosine','timings')}))


if __name__ == '__main__':
    main()
