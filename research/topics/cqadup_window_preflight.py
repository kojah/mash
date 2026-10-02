"""Pinned complete-corpus window preflight before semantic retrieval outcomes."""
import argparse
from dataclasses import asdict
import hashlib
import json
from pathlib import Path
import time

import numpy as np

from cqadup_retrieval import CAPTURE, COHORT
from cqadup_source import load, partition
from embedding_windows import Field, unbounded_tokenizer, windows
from external_context import compare


MODEL = 'afdb6f1a0e45b715d0bb9b11772f032c399babd23bfc31fed1c170afc848bdb1'
TOKENIZER = 'da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0'


def preflight(root, snapshot):
    started = time.perf_counter()
    model_raw = (snapshot / 'onnx/model_quantized.onnx').read_bytes()
    tokenizer_raw = (snapshot / 'tokenizer.json').read_bytes()
    if hashlib.sha256(model_raw).hexdigest() != MODEL or hashlib.sha256(tokenizer_raw).hexdigest() != TOKENIZER:
        raise ValueError('explicit pinned installed model/tokenizer required; no downloads')
    tokenizer = unbounded_tokenizer(tokenizer_raw.decode())
    documents, queries, references, _ = load(root, CAPTURE)
    raw = (root / 'cohort-first.json').read_bytes()
    if hashlib.sha256(raw).hexdigest() != COHORT or partition(documents, queries, references) != json.loads(raw)['cohort']:
        raise ValueError('pinned complete prospective cohort required')
    query_ids = sorted(q for group in json.loads(raw)['cohort']['phases']['development'] for q in group['queries'])
    ledgers = {field: [] for field in Field}
    whole_tokens = {field: [] for field in Field}
    source_fields = [(documents[k], ((Field.TITLE, documents[k].title), (Field.BODY, documents[k].text)))
                     for k in sorted(documents)]
    source_fields += [(queries[k], ((Field.QUERY, queries[k].text),)) for k in query_ids]
    for source, fields in source_fields:
        for field, text in fields:
            full = tokenizer.encode(text).ids if text.strip() else []
            units = windows(tokenizer, source.id, field, text)
            whole_tokens[field].append(len(full))
            ledgers[field].append({'source': source.id, 'source_text_sha256': hashlib.sha256(text.encode()).hexdigest(),
                                   'characters': len(text), 'bytes': len(text.encode()),
                                   'whole_tokens': len(full), 'windows': [asdict(w) for w in units]})
    summary = {}
    for field, records in ledgers.items():
        lengths = whole_tokens[field]
        summary[field] = {'sources': len(records), 'empty': sum(n == 0 for n in lengths),
                          'over256': sum(n > 256 for n in lengths), 'over512': sum(n > 512 for n in lengths),
                          'p50_tokens': float(np.median(lengths)), 'p95_tokens': float(np.percentile(lengths, 95)),
                          'max_tokens': max(lengths),
                          'windows': sum(len(r['windows']) for r in records),
                          'max_windows': max(len(r['windows']) for r in records),
                          'payload_tokens': sum(w['payload_tokens'] for r in records for w in r['windows'])}
    return {'version': 1, 'scope': 'complete-corpus-development-query-window-preflight-no-outcomes',
            'capture_sha256': CAPTURE, 'cohort_sha256': COHORT, 'model_sha256': MODEL, 'tokenizer_sha256': TOKENIZER,
            'code_pins': {n: hashlib.sha256(Path(__file__).with_name(n).read_bytes()).hexdigest() for n in
                          ('cqadup_window_preflight.py', 'embedding_windows.py', 'cqadup_source.py', 'uv.lock')},
            'maximum_window_tokens': 256, 'summary': summary, 'ledgers': ledgers,
            'encoder_calls': 0, 'retrieval_outcomes': None, 'reserved_queries_tokenized': 0,
            'timings': {'preflight_ms': (time.perf_counter() - started) * 1000}}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--root', type=Path, required=True)
    p.add_argument('--snapshot', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--compare', type=Path)
    args = p.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git output required')
    result = preflight(args.root, args.snapshot)
    # Round-trip arrays/types exactly as persisted in the cross-language schema.
    result = json.loads(json.dumps(result, allow_nan=False))
    if args.compare:
        compare(json.loads(args.compare.read_text()), result)
    with args.output.open('x') as file:
        json.dump(result, file, sort_keys=True, allow_nan=False)
    print(json.dumps({'stage': 'complete', 'summary': result['summary'], 'timings': result['timings']}))


if __name__ == '__main__':
    main()
