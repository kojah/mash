"""Bounded context and native identity audit for pinned fresh review packets."""
import argparse
from collections import Counter
import json
from pathlib import Path
import sqlite3

from data import load
from episodes import Turn, source_hash
from native_audit import Provider, read_native, verify
from packet_review import Origin, validate
from preceding_context import Message, Role, identity, preceding
from reference_review import digest


def capture_case(case, messages, anchors):
    visible = [m for m in messages if m.role == Role.USER and m.searchable.strip()]
    turns = tuple(Turn(m.id, m.ordinal, m.searchable, None) for m in visible)
    if source_hash(turns) != case['source_sha256'] or len(visible) != len(case['turns']):
        raise ValueError('complete source-user sequence changed')
    for m, recorded in zip(visible, case['turns']):
        if (m.id, m.ordinal, digest(m.searchable.encode())) != (recorded['id'], recorded['ordinal'], recorded['text_sha256']):
            raise ValueError('packet anchor identity changed')
    if len(set(anchors)) != len(anchors) or not set(anchors) <= {m.ordinal for m in visible}:
        raise ValueError('unexpected or duplicate context anchors')
    return [preceding(messages, ordinal, maximum_users=3) for ordinal in anchors]


def native_evidence(contexts, messages, native):
    by_ordinal = {m.ordinal: m for m in messages}
    ordinals = sorted({entry['ordinal'] for context in contexts
                       for entry in [context['anchor'], *context['display']]})
    results = []
    for ordinal in ordinals:
        m = by_ordinal[ordinal]
        results.append({'identity': identity(m), **verify(native, m.id, m.role, m.canonical)})
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('database', 'partition-report', 'packets', 'review', 'output'):
        parser.add_argument(f'--{name}', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('use fresh private output')
    raw = [p.read_bytes() for p in (args.packets, args.review)]
    judgments = validate(*raw)
    packet = json.loads(raw[0])
    dataset = load(args.database, args.partition_report)
    if (packet['generation'], packet['input_fingerprint'], packet['partition_sha256']) != (
            dataset.generation, dataset.fingerprint, dataset.partition_sha256):
        raise ValueError('packet generation/partition changed')
    eligible = {t.id: t for t in dataset.threads}
    cases = []
    with sqlite3.connect(args.database.resolve().as_uri() + '?mode=ro', uri=True) as connection:
        connection.execute('BEGIN')
        for case in packet['cases']:
            thread = eligible.get(case['thread'])
            if thread is None or thread.group[0] % 5 == 0 or thread.group.hex() != case['source_group']:
                raise ValueError('exact canonical development group required')
            rows = connection.execute('SELECT id,ordinal,role,context_parent,content,COALESCE(search_projection,content) '
                                      'FROM messages WHERE conversation_id=? ORDER BY ordinal', (thread.id,))
            messages = tuple(Message(row[0], row[1], Role(row[2]), *row[3:]) for row in rows)
            anchors = [j.ordinal for j in judgments if j.thread == thread.id and j.origin == Origin.REFERENCE]
            contexts = capture_case(case, messages, anchors)
            native_source, evidence = None, []
            if contexts:
                path = Path(connection.execute('SELECT source_path FROM conversations WHERE id=?', (thread.id,)).fetchone()[0])
                native_source, native = read_native(path, Provider(case['provider']), thread.id)
                evidence = native_evidence(contexts, messages, native)
            cases.append({'thread': thread.id, 'provider': case['provider'], 'source_group': case['source_group'],
                          'source_sha256': case['source_sha256'], 'contexts': contexts,
                          'native_source_sha256': native_source, 'native_identity_audit': evidence})
    report = {'schema_version': 1, 'scope': 'development-packet-referential-contexts',
              'generation': dataset.generation, 'dataset_fingerprint': dataset.fingerprint,
              'packets_sha256': digest(raw[0]), 'source_review_sha256': digest(raw[1]),
              'code_sha256': {n: digest(Path(__file__).with_name(n).read_bytes()) for n in
                              ('packet_contexts.py', 'preceding_context.py', 'native_audit.py', 'packet_review.py',
                               'reference_review.py', 'review_packets.py', 'episodes.py', 'data.py', 'uv.lock')},
              'selection': 'prior exposed origin-review referential anchors only; no inference',
              'cases': cases, 'limitations': ['selected cohort contains referential anchors in Codex only',
                  'current native canonical identity/content verification, not historical whole-file equivalence',
                  'stored predecessor ancestry/order; native order, tool bodies and branch completeness not independently verified',
                  'role does not prove human substantive intent; display guard is not a general secret scanner',
                  'bounded display may omit referents; no semantic resolution or production promotion']}
    with args.output.open('x') as output:
        json.dump(report, output, indent=2, ensure_ascii=False)
    print(json.dumps({'cases': len(cases), 'anchors': sum(len(c['contexts']) for c in cases),
                      'native_identity_status': Counter(r['status'] for c in cases for r in c['native_identity_audit']),
                      'stops': Counter(s['stop'] for c in cases for s in c['contexts']),
                      'omitted_records': sum(s['omitted_records'] for c in cases for s in c['contexts'])}))


if __name__ == '__main__':
    main()
