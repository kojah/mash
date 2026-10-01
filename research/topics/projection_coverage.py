"""Read-only canonical/projection/vector coverage ledger; no inference or readiness repair."""
import argparse
from collections import Counter, defaultdict
from dataclasses import dataclass
from enum import StrEnum
import hashlib
import json
from pathlib import Path
import sqlite3
import time

from data import decode_vector, load
from external_context import compare
from reference_review import digest
from stored_frequency import CAPTURE
from stored_vector_reuse import FINGERPRINT, GENERATION


class Projection(StrEnum):
    CANONICAL = 'canonical-no-override'
    EMPTY = 'explicit-empty'
    WHITESPACE = 'explicit-whitespace'
    IDENTITY = 'explicit-identical'
    CHANGED = 'explicit-changed'


class Embedding(StrEnum):
    COMPATIBLE = 'compatible'
    INCOMPATIBLE = 'incompatible'
    MISSING = 'missing-searchable'
    EXCLUDED = 'excluded-no-vector'
    UNEXPECTED = 'excluded-has-vector'


@dataclass(frozen=True)
class State:
    projection: Projection
    embedding: Embedding
    sql_searchable: bool
    nonblank: bool
    pending: bool


def classify(content, projection, raw, generation, dimensions, norm, pending):
    if not isinstance(content, str) or (projection is not None and not isinstance(projection, str)) or type(pending) is not bool:
        raise ValueError('typed canonical/projection/pending state required')
    text = content if projection is None else projection
    kind = (Projection.CANONICAL if projection is None else Projection.EMPTY if projection == '' else
            Projection.WHITESPACE if not projection.strip() else Projection.IDENTITY if projection == content else Projection.CHANGED)
    if raw is None:
        embedding = Embedding.MISSING if text != '' else Embedding.EXCLUDED
    else:
        try:
            if generation != GENERATION:
                raise ValueError('generation mismatch')
            decode_vector(raw, dimensions, norm)
            embedding = Embedding.COMPATIBLE if text != '' else Embedding.UNEXPECTED
        except (ValueError, TypeError):
            embedding = Embedding.INCOMPATIBLE
    return State(kind, embedding, text != '', bool(text.strip()), pending)


def account(counter, state):
    counter['canonical_records'] += 1
    counter['sql_searchable'] += state.sql_searchable
    counter['nonblank_projection'] += state.nonblank
    counter['pending'] += state.pending
    counter['projection:' + state.projection] += 1
    counter['embedding:' + state.embedding] += 1


def experiment(database, partition, capture):
    started = time.perf_counter()
    dataset = load(database, partition)
    if dataset.fingerprint != FINGERPRINT or dataset.generation != GENERATION:
        raise ValueError('original frozen research dataset required')
    capture_raw = capture.read_bytes()
    if digest(capture_raw) != CAPTURE:
        raise ValueError('original stored-vector selection required')
    selected = json.loads(capture_raw)['records']
    totals, providers, threads = Counter(), defaultdict(Counter), defaultdict(Counter)
    ledger = hashlib.sha256()
    count = source_bytes = 0
    with sqlite3.connect(database.resolve().as_uri() + '?mode=ro', uri=True) as connection:
        connection.execute('BEGIN')
        for row in connection.execute(
            'SELECT c.id,c.provider,m.id,m.ordinal,m.role,m.content,m.search_projection,'
            'e.vector,e.generation,e.dimensions,e.norm,p.message_id IS NOT NULL '
            'FROM messages m JOIN conversations c ON c.id=m.conversation_id '
            'LEFT JOIN message_embeddings e ON e.message_id=m.id '
            'LEFT JOIN pending_embeddings p ON p.message_id=m.id ORDER BY c.id,m.ordinal,m.id'):
            thread, provider, key, ordinal, role, content, projection, raw, space, dimensions, norm, pending = row
            count += 1
            encoded = json.dumps([thread, provider, key, ordinal, role, content, projection,
                                  digest(raw) if raw is not None else None, space, dimensions, norm, bool(pending)],
                                 ensure_ascii=False, separators=(',', ':'), allow_nan=False).encode()
            source_bytes += len(encoded)
            if count > 500_000 or len(encoded) > 64 * 1024 * 1024 or source_bytes > 2 * 1024**3:
                raise ValueError('complete coverage audit exceeds bounds; no partial result')
            ledger.update(len(encoded).to_bytes(8, 'little'))
            ledger.update(encoded)
            state = classify(content, projection, raw, space, dimensions, norm, bool(pending))
            account(totals, state)
            account(providers[provider + ':' + role], state)
            if thread in selected and role == 'user':
                account(threads[thread], state)
        if set(threads) != set(selected):
            raise ValueError('selected source threads missing from complete ledger')
        derived = connection.execute('SELECT search_dirty,semantic_ready_generation FROM derived_state WHERE singleton=1').fetchone()
        packed = connection.execute('SELECT COALESCE(sum(vector_count),0) FROM semantic_chunks WHERE generation=?', (GENERATION,)).fetchone()[0]
        orphaned = connection.execute('SELECT count(*) FROM message_embeddings e LEFT JOIN messages m ON m.id=e.message_id WHERE m.id IS NULL').fetchone()[0]
    return {'version': 1, 'scope': 'read-only-full-canonical-projection-vector-audit',
            'generation': GENERATION, 'fingerprint': FINGERPRINT, 'partition_sha256': dataset.partition_sha256,
            'selection_sha256': CAPTURE, 'complete_ledger_sha256': ledger.hexdigest(), 'ledger_bytes': source_bytes,
            'code_pins': {name: digest(Path(__file__).with_name(name).read_bytes()) for name in
                          ('projection_coverage.py', 'data.py', 'stored_frequency.py', 'stored_vector_reuse.py', 'uv.lock')},
            'totals': dict(totals), 'provider_roles': {k: dict(v) for k, v in providers.items()},
            'selected_user_threads': {k: dict(v) for k, v in threads.items()},
            'readiness': {'search_dirty': bool(derived[0]), 'marker_matches': derived[1] == GENERATION,
                          'marker_present': derived[1] is not None, 'packed_vectors': packed,
                          'packed_matches_projection_count': packed == totals['sql_searchable'], 'orphaned_vectors': orphaned},
            'timings': {'total_ms': (time.perf_counter() - started) * 1000}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('database', 'partition', 'capture', 'output'):
        parser.add_argument('--' + name, required=True, type=Path)
    parser.add_argument('--compare', type=Path)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git audit output required')
    result = experiment(args.database, args.partition, args.capture)
    if args.compare:
        compare(json.loads(args.compare.read_text()), result)
    with args.output.open('x') as output:
        json.dump(result, output, sort_keys=True, allow_nan=False)
    print(json.dumps({'totals': result['totals'], 'readiness': result['readiness'], 'timings': result['timings']}))


if __name__ == '__main__':
    main()
