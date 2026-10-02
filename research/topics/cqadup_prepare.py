"""Prepare pinned duplicate-family cohorts before any retrieval outcomes."""
import argparse
import hashlib
import json
from pathlib import Path
import time

from cqadup_source import load, partition
from external_context import compare


def prepare(root, capture_pin):
    started = time.perf_counter()
    documents, queries, references, source_summary = load(root, capture_pin)
    cohort = partition(documents, queries, references)
    judged = {r.query for r in references}
    if judged != set(queries):
        raise ValueError('complete reference-supported query population required')
    return {'version': 1, 'scope': 'BEIR-programmers-duplicate-family-preparation-no-outcomes',
            'capture_sha256': capture_pin, 'source_summary': source_summary,
            'references': {'positive_links': len(references), 'judged_queries': len(judged),
                           'relevance_expanded_by_transitivity': False},
            'cohort': cohort, 'retrieval_candidate_pool': 'complete-original-corpus-no-gold-negative-sampling',
            'feature_fields': {'corpus': ['title', 'text'], 'queries': ['text'], 'metadata': 'opaque-excluded'},
            'encoder_calls': 0, 'retrieval_outcomes': None,
            'code_pins': {name: hashlib.sha256(Path(__file__).with_name(name).read_bytes()).hexdigest()
                          for name in ('cqadup_prepare.py', 'cqadup_source.py', 'cqadup_capture.py', 'uv.lock')},
            'timings': {'preparation_ms': (time.perf_counter() - started) * 1000}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--capture-sha256', required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--compare', type=Path)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git preparation output required')
    result = prepare(args.root, args.capture_sha256)
    if args.compare:
        compare(json.loads(args.compare.read_text()), result)
    with args.output.open('x') as file:
        json.dump(result, file, sort_keys=True, allow_nan=False)
    print(json.dumps({'stage': 'complete', 'sources': result['source_summary'],
                      'references': result['references'], 'phases': result['cohort']['summary'],
                      'timings': result['timings']}))


if __name__ == '__main__':
    main()
