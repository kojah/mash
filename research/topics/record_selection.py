"""Zero-inference reference-record retrieval; not accepted-passage selection."""
import argparse
from dataclasses import dataclass
import json
from pathlib import Path
import sqlite3
import time

import numpy as np
from tokenizers import Tokenizer

from data import decode_vector, load, normalized
from packet_contexts import capture_case
from packet_reference_review import validate
from preceding_context import Message, Role, identity
from reference_review import digest
from span_selection import Query


@dataclass(frozen=True)
class Record:
    id: str
    ordinal: int
    tokens: int
    vector: np.ndarray | None


def eligible(record):
    return record.vector is not None and 2 <= record.tokens <= 512


def rank(candidates, queries=None):
    if queries is None:
        return [{'id': r.id, 'ordinal': r.ordinal, 'cosine': None}
                for r in sorted(candidates, key=lambda r: (-r.ordinal, r.id))]
    if not queries or any(not eligible(r) for r in (*candidates, *queries)):
        return []
    summed = np.mean([r.vector for r in queries], axis=0)
    if np.linalg.norm(summed) <= np.finfo(np.float64).eps:
        return []
    direction = normalized(summed)
    scored = [{'id': r.id, 'ordinal': r.ordinal, 'cosine': float(np.clip(direction @ r.vector, -1, 1))}
              for r in candidates]
    return sorted(scored, key=lambda r: (-r['cosine'], r['ordinal'], r['id']))


def query_records(anchor, context, records, method):
    if method == Query.CURRENT:
        return [records[anchor['id']]]
    users = [records[e['id']] for e in context['display'] if e['role'] == Role.USER]
    if not users or any(e['display_status'] != 'included' for e in context['display'] if e['role'] == Role.USER):
        return []
    return users[-1:] if method == Query.PREVIOUS else [*users, records[anchor['id']]]


def hits(ranking, ids):
    return {f'hit_at_{k}': bool({r['id'] for r in ranking[:k]} & ids) for k in (1, 3)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('database', 'partition-report', 'packets', 'source-review', 'contexts', 'reference-review', 'tokenizer', 'output'):
        parser.add_argument(f'--{name}', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('use fresh private output')
    started = time.perf_counter()
    raw = {name: getattr(args, name).read_bytes() for name in ('packets', 'source_review', 'contexts', 'reference_review', 'tokenizer')}
    if max(map(len, raw.values())) > 16 * 1024 * 1024:
        raise ValueError('record-selection input exceeds cap')
    annotation = json.loads(raw['reference_review'])
    validate(raw['contexts'], raw['packets'], raw['source_review'], annotation)
    packet, captured = json.loads(raw['packets']), json.loads(raw['contexts'])
    dataset = load(args.database, args.partition_report)
    if (dataset.fingerprint != captured['dataset_fingerprint'] or packet['generation'] != dataset.generation
            or packet['partition_sha256'] != dataset.partition_sha256):
        raise ValueError('record-selection generation/partition changed')
    tokenizer = Tokenizer.from_str(raw['tokenizer'].decode())
    tokenizer.no_truncation()
    tokenizer.no_padding()
    cases = {c['thread']: c for c in packet['cases']}
    eligible_groups = {t.id: t for t in dataset.threads}
    reviewed = {(j['thread'], j['ordinal']): j for j in annotation['judgments']}
    reports, inventory = [], []
    preflight_ms = 0
    with sqlite3.connect(args.database.resolve().as_uri() + '?mode=ro', uri=True) as connection:
        connection.execute('BEGIN')
        for case in captured['cases']:
            original = cases[case['thread']]
            group = eligible_groups[case['thread']]
            if group.group[0] % 5 == 0 or group.group.hex() != original['source_group']:
                raise ValueError('exact canonical development source required')
            rows = connection.execute('SELECT id,ordinal,role,context_parent,content,COALESCE(search_projection,content) '
                                      'FROM messages WHERE conversation_id=? ORDER BY ordinal', (case['thread'],)).fetchall()
            messages = tuple(Message(r[0], r[1], Role(r[2]), *r[3:]) for r in rows)
            contexts = case['controls']['raw64']['contexts']
            if capture_case(original, messages, [s['anchor']['ordinal'] for s in contexts]) != contexts:
                raise ValueError('frozen original record context changed')
            required = {e['id']: e for s in contexts for e in [s['anchor'], *s['display']]}
            by_id = {m.id: m for m in messages}
            records = {}
            for key, expected in required.items():
                m = by_id[key]
                if identity(m) != {k: expected[k] for k in identity(m)}:
                    raise ValueError('stored source identity/projection changed')
                stored = connection.execute('SELECT vector,generation,dimensions,norm FROM message_embeddings WHERE message_id=?', (key,)).fetchone()
                vector = None
                if stored is not None:
                    if stored[1] != dataset.generation:
                        raise ValueError('stored vector generation changed')
                    vector = decode_vector(stored[0], stored[2], stored[3])
                preflight_start = time.perf_counter()
                tokens = len(tokenizer.encode(m.searchable).ids)
                preflight_ms += (time.perf_counter() - preflight_start) * 1000
                records[key] = Record(key, m.ordinal, tokens, vector)
                inventory.append({'thread': case['thread'], 'identity': identity(m), 'tokens': tokens,
                                  'stored_vector_sha256': digest(stored[0]) if stored else None,
                                  'full_window_eligible': eligible(records[key])})
            for context in contexts:
                anchor = context['anchor']
                selection_start = time.perf_counter()
                candidates = [records[e['id']] for e in context['display']
                              if e['role'] == Role.ASSISTANT and e['display_status'] == 'included']
                full = [r for r in candidates if eligible(r)]
                methods = {'recency-all': rank(candidates), 'recency-full-window': rank(full)}
                statuses = {'recency-all': 'ranked' if candidates else 'no-candidates',
                            'recency-full-window': 'ranked' if full else 'no-candidates'}
                for query in Query:
                    inputs = query_records(anchor, context, records, query)
                    methods[query] = rank(full, inputs)
                    statuses[query] = 'ranked' if methods[query] else 'missing-or-ineligible-query/candidates'
                selection_ms = (time.perf_counter() - selection_start) * 1000
                # Scorers above receive no judgments. Exact cited-assistant retrieval
                # is evaluated below, without calling uncited records false topics.
                judgment = reviewed[case['thread'], anchor['ordinal']]
                gold = {e['id'] for e in judgment['evidence'] if e['role'] == Role.ASSISTANT}
                supported = bool(gold) and gold <= {r.id for r in full}
                evaluations = {name: {'status': statuses[name], 'ranking': ranking,
                    'cited_record_hits': hits(ranking, gold) if gold and (name == 'recency-all' or supported) and ranking else None}
                    for name, ranking in methods.items()}
                reports.append({'thread': case['thread'], 'anchor': anchor, 'resolution': judgment['resolution'], 'selection_ms': selection_ms,
                                'cited_assistant_ids': sorted(gold), 'full_window_reference_supported': supported,
                                'all_candidates': len(candidates), 'full_window_candidates': len(full), 'methods': evaluations})
    report = {'schema_version': 1, 'scope': 'development-stored-reference-record-selection',
              'dataset_fingerprint': dataset.fingerprint, 'generation': dataset.generation,
              'input_sha256': {name: digest(value) for name, value in raw.items()}, 'inventory': inventory,
              'reports': reports, 'inference_calls': 0, 'preflight_ms': preflight_ms,
              'parameters': {'candidate_role': 'assistant', 'context': 'unchanged raw64', 'token_window': [2, 512],
                             'query_modes': list(Query), 'evaluated_cutoffs': [1, 3], 'precision': 'float64',
                             'vectors': 'normalized stored symmetric signed-byte directions',
                             'similarity_ties': 'ascending source ordinal, then identity',
                             'recency_ties': 'descending source ordinal, then ascending identity',
                             'canceling_queries': 'abstain at float64 epsilon'},
              'code_sha256': {n: digest(Path(__file__).with_name(n).read_bytes()) for n in
                             ('record_selection.py', 'packet_contexts.py', 'packet_reference_review.py', 'reference_review.py',
                              'preceding_context.py', 'data.py', 'episodes.py', 'span_selection.py', 'uv.lock')},
              'ranking_ms_with_validation': (time.perf_counter() - started) * 1000,
              'limitations': ['oracle referential source admission and cited-record evaluation; reviewers never scorer inputs',
                  'only two exposed Codex groups; cited-record hit is not semantic precision or accepted-passage quality',
                  'stored generation/dimensions/norm validated; over-window vectors excluded from whole-source ranking',
                  'all-source recency has different coverage; matched comparisons require supported queries/references',
                  'no new inference/truncation, independent intent judgment, ingestion gate or production promotion']}
    with args.output.open('x') as output:
        json.dump(report, output, indent=2, allow_nan=False)
    print(json.dumps({'records': len(inventory), 'anchors': len(reports), 'inference_calls': 0,
                      'full_window_reference_supported': sum(r['full_window_reference_supported'] for r in reports),
                      'ranking_ms_with_validation': report['ranking_ms_with_validation']}))


if __name__ == '__main__':
    main()
