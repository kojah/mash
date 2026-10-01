"""Zero-inference, source-pinned message-vector ranking; project scope is only a proxy."""
import argparse
from dataclasses import dataclass
import hashlib
import json
from pathlib import Path
import sqlite3
import time

import numpy as np

from data import decode_vector, load, normalized
from external_context import compare
from question_transfer import rank_metrics
from reference_review import digest
from review_packets import exclusions


GENERATION = 'ca9c7766f955402eb34db3c324da24a512e1118de44e3d9aa9a0faeeac0da5f4'
FINGERPRINT = '2d219b5cb5947231612d4612e6f930e6d6ac740ea00d9cf2eaf9606c0bc772a2'
SEED = 'mash-stored-vector-reuse-v1-20261002'


@dataclass(frozen=True)
class MessageVector:
    identity: str
    ordinal: int
    input_sha256: str
    vector_sha256: str
    vector: np.ndarray


def messages(connection, identity, generation):
    rows = connection.execute(
        'SELECT m.id,m.ordinal,COALESCE(m.search_projection,m.content),'
        'e.vector,e.generation,e.dimensions,e.norm FROM messages m '
        'JOIN message_embeddings e ON e.message_id=m.id '
        "WHERE m.conversation_id=? AND m.role='user' ORDER BY m.ordinal,m.id", (identity,))
    result, seen, duplicates = [], {}, 0
    for key, ordinal, text, raw, space, dimensions, norm in rows:
        if space != generation or type(ordinal) is not int:
            raise ValueError('stored message contract drift')
        vector = decode_vector(raw, dimensions, norm)
        if not text.strip():
            continue
        if text in seen:
            if raw != seen[text]:
                raise ValueError('identical stored input has conflicting vectors')
            duplicates += 1
            continue
        seen[text] = raw
        result.append(MessageVector(key, ordinal, digest(text.encode()), digest(raw), vector))
    return tuple(result), duplicates


def scores(left, right):
    a, b = np.asarray(left, dtype=np.float64), np.asarray(right, dtype=np.float64)
    if (a.ndim != 2 or b.ndim != 2 or a.shape[1] != b.shape[1] or not len(a) or not len(b)
            or not np.isfinite(a).all() or not np.isfinite(b).all()
            or not np.allclose(np.linalg.norm(a, axis=1), 1., rtol=1e-12, atol=1e-12)
            or not np.allclose(np.linalg.norm(b, axis=1), 1., rtol=1e-12, atol=1e-12)):
        raise ValueError('finite unit-vector matrices required')
    similarities = np.clip(a @ b.T, -1, 1)
    return {'mean': float(np.clip(normalized(a.mean(0)) @ normalized(b.mean(0)), -1, 1)),
            'maximum': float(similarities.max()),
            'bidirectional-nearest-mean': float((similarities.max(1).mean() + similarities.max(0).mean()) / 2)}


def packet_exclusions(path, groups, dataset):
    raw = path.read_bytes()
    if len(raw) > 16 * 1024 * 1024:
        raise ValueError('exclusion packet exceeds cap')
    packet = json.loads(raw)
    if (packet['schema_version'] != 1 or packet['generation'] != dataset.generation
            or packet['input_fingerprint'] != dataset.fingerprint
            or packet['partition_sha256'] != dataset.partition_sha256):
        raise ValueError('exclusion packet contract drift')
    excluded = set()
    for case in packet['cases']:
        group = groups.get(case['thread'])
        if group is None or bytes(group).hex() != case['source_group']:
            raise ValueError('exclusion packet source group drift')
        excluded.add(bytes(group))
    return excluded, {'sha256': digest(raw), 'cases': len(packet['cases'])}


def experiment(database, partition, annotation_paths, packet_path):
    started = time.perf_counter()
    dataset = load(database, partition)
    if dataset.generation != GENERATION or dataset.fingerprint != FINGERPRINT:
        raise ValueError('original frozen stored-vector dataset required')
    groups = json.loads(partition.read_text())['partition']['source_ids']
    excluded, manifest = exclusions(tuple(annotation_paths), groups, dataset.generation)
    extra, packet_manifest = packet_exclusions(packet_path, groups, dataset)
    excluded.update(extra)
    manifest.append(packet_manifest)
    before = time.perf_counter()
    candidates, vectors, duplicate_counts, user_counts = [], {}, {}, {}
    with sqlite3.connect(database.resolve().as_uri() + '?mode=ro', uri=True) as connection:
        connection.execute('BEGIN')
        for thread in dataset.threads:
            if thread.group in excluded:
                continue
            values, duplicates = messages(connection, thread.id, dataset.generation)
            if not 2 <= len(values) <= 128:
                continue
            candidates.append(thread)
            vectors[thread.id], duplicate_counts[thread.id] = values, duplicates
        candidates.sort(key=lambda t: hashlib.sha256(SEED.encode() + t.group).digest())
        if len(candidates) < 128:
            raise ValueError('insufficient bounded source candidates; no resampling')
        selected = candidates[:128]
        # Metadata is read only after source-only selection, never a representation.
        references = {}
        for thread in selected:
            references[thread.id] = connection.execute('SELECT project FROM conversations WHERE id=?', (thread.id,)).fetchone()[0]
            user_counts[thread.id] = connection.execute("SELECT count(*) FROM messages WHERE conversation_id=? AND role='user'", (thread.id,)).fetchone()[0]
    loading_ms = (time.perf_counter() - before) * 1000
    records = {t.id: {'group': t.group.hex(), 'provider': t.provider,
                     'user_messages': user_counts[t.id], 'duplicate_inputs': duplicate_counts[t.id],
                     'project_sha256': digest(references[t.id].encode()) if isinstance(references[t.id], str) and references[t.id].strip() else None,
                     'messages': [{'identity': v.identity, 'ordinal': v.ordinal, 'input_sha256': v.input_sha256,
                                   'stored_vector_sha256': v.vector_sha256, 'direction': v.vector.tolist()}
                                  for v in vectors[t.id]]} for t in selected}
    rows, values = [], {name: [] for name in ('mean', 'maximum', 'bidirectional-nearest-mean')}
    unsupported = operations = 0
    before = time.perf_counter()
    for n, left in enumerate(selected):
        for right in selected[n + 1:]:
            a, b = left.id, right.id
            if records[a]['project_sha256'] is None or records[b]['project_sha256'] is None:
                unsupported += 1
                continue
            rows.append({'left': a, 'right': b, 'tag_overlap': references[a] == references[b]})
            pair = scores([v.vector for v in vectors[a]], [v.vector for v in vectors[b]])
            operations += len(vectors[a]) * len(vectors[b])
            for name, value in pair.items():
                values[name].append(value)
    scoring_ms = (time.perf_counter() - before) * 1000
    if not rows:
        raise ValueError('no supported reference pairs; no replacement sample')
    return {'version': 1, 'scope': 'exposed-stored-message-project-proxy-no-topic-selection',
            'seed': SEED, 'generation': dataset.generation, 'fingerprint': dataset.fingerprint,
            'partition_sha256': dataset.partition_sha256, 'excluded_annotations': manifest,
            'excluded_groups': len(excluded), 'eligible_candidates': len(candidates),
            'policy': {'selected': 128, 'minimum_distinct_vectors': 2, 'maximum_distinct_vectors': 128,
                       'reference': 'exact-nonempty-project-identity', 'inference_calls': 0},
            'code_pins': {name: digest(Path(__file__).with_name(name).read_bytes()) for name in
                          ('stored_vector_reuse.py', 'data.py', 'review_packets.py', 'reference_review.py', 'question_transfer.py', 'uv.lock')},
            'records': records, 'rows': rows, 'scores': values,
            'metrics': {name: rank_metrics(rows, values[name]) for name in values},
            'unsupported_pairs': unsupported, 'cross_message_scores': operations,
            'timings': {'loading_ms': loading_ms, 'scoring_ms': scoring_ms,
                        'total_ms': (time.perf_counter() - started) * 1000}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('database', 'partition', 'output'):
        parser.add_argument('--' + name, required=True, type=Path)
    parser.add_argument('--exclude-annotations', action='append', required=True, type=Path)
    parser.add_argument('--exclude-packet', required=True, type=Path)
    parser.add_argument('--compare', type=Path)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git output required')
    result = experiment(args.database, args.partition, args.exclude_annotations, args.exclude_packet)
    if args.compare:
        compare(json.loads(args.compare.read_text()), result)
    with args.output.open('x') as output:
        json.dump(result, output, sort_keys=True, allow_nan=False)
    print(json.dumps({'metrics': result['metrics'], 'timings': result['timings'],
                      'candidates': result['eligible_candidates'], 'unsupported_pairs': result['unsupported_pairs']}))


if __name__ == '__main__':
    main()
