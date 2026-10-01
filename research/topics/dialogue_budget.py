"""Changed-scope visible-dialogue traversal versus exact frozen raw-record control."""
import argparse
from collections import Counter
from enum import StrEnum
import json
from pathlib import Path
import sqlite3
import time

from data import load
from native_audit import Provider, read_native
from packet_contexts import capture_case, native_evidence
from packet_review import Origin, validate
from preceding_context import Message, Role, preceding
from reference_review import digest


class Control(StrEnum):
    RAW = 'raw64'
    VISIBLE = 'visible16-raw4096'


def compare_case(case, messages, anchors, expected):
    started = time.perf_counter()
    original = capture_case(case, messages, anchors)
    raw_ms = (time.perf_counter() - started) * 1000
    if original != expected:
        raise ValueError('original frozen context control changed')
    started = time.perf_counter()
    visible = [preceding(messages, ordinal, maximum_users=3, maximum_records=4096, maximum_visible=16)
               for ordinal in anchors]
    visible_ms = (time.perf_counter() - started) * 1000
    return {Control.RAW: {'contexts': original, 'capture_ms': raw_ms},
            Control.VISIBLE: {'contexts': visible, 'capture_ms': visible_ms}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('database', 'partition-report', 'packets', 'review', 'baseline', 'output'):
        parser.add_argument(f'--{name}', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('use fresh private output')
    raw = [p.read_bytes() for p in (args.packets, args.review, args.baseline)]
    if max(map(len, raw)) > 16 * 1024 * 1024:
        raise ValueError('context inputs exceed cap')
    judgments = validate(*raw[:2])
    packet, baseline = json.loads(raw[0]), json.loads(raw[2])
    if (baseline['schema_version'] != 1 or baseline['scope'] != 'development-packet-referential-contexts'
            or baseline['packets_sha256'] != digest(raw[0]) or baseline['source_review_sha256'] != digest(raw[1])):
        raise ValueError('frozen baseline source changed')
    dataset = load(args.database, args.partition_report)
    if (packet['generation'], packet['input_fingerprint'], packet['partition_sha256']) != (
            dataset.generation, dataset.fingerprint, dataset.partition_sha256):
        raise ValueError('packet generation/partition changed')
    frozen = {c['thread']: c for c in baseline['cases']}
    if len(frozen) != len(baseline['cases']) or set(frozen) != {c['thread'] for c in packet['cases']}:
        raise ValueError('baseline case scope changed')
    eligible = {t.id: t for t in dataset.threads}
    cases = []
    with sqlite3.connect(args.database.resolve().as_uri() + '?mode=ro', uri=True) as connection:
        connection.execute('BEGIN')
        for case in packet['cases']:
            thread = eligible.get(case['thread'])
            if (thread is None or thread.group[0] % 5 == 0 or thread.group.hex() != case['source_group']
                    or thread.provider != case['provider']):
                raise ValueError('exact canonical development group/provider required')
            rows = connection.execute('SELECT id,ordinal,role,context_parent,content,COALESCE(search_projection,content) '
                                      'FROM messages WHERE conversation_id=? ORDER BY ordinal', (thread.id,))
            messages = tuple(Message(row[0], row[1], Role(row[2]), *row[3:]) for row in rows)
            anchors = [j.ordinal for j in judgments if j.thread == thread.id and j.origin == Origin.REFERENCE]
            controls = compare_case(case, messages, anchors, frozen[thread.id]['contexts'])
            native_source, evidence = None, []
            if anchors:
                path = Path(connection.execute('SELECT source_path FROM conversations WHERE id=?', (thread.id,)).fetchone()[0])
                native_source, native = read_native(path, Provider(case['provider']), thread.id)
                if native_source != frozen[thread.id]['native_source_sha256']:
                    raise ValueError('native source bytes changed since baseline')
                evidence = native_evidence(controls[Control.VISIBLE]['contexts'], messages, native)
            cases.append({'thread': thread.id, 'provider': case['provider'], 'source_sha256': case['source_sha256'],
                          'controls': controls, 'native_source_sha256': native_source, 'native_identity_audit': evidence})
    report = {'schema_version': 1, 'scope': 'development-visible-dialogue-budget-control',
              'input_sha256': dict(zip(('packets', 'review', 'baseline'), map(digest, raw))),
              'dataset_fingerprint': dataset.fingerprint, 'cases': cases,
              'code_sha256': {n: digest(Path(__file__).with_name(n).read_bytes()) for n in
                             ('dialogue_budget.py', 'packet_contexts.py', 'preceding_context.py', 'native_audit.py',
                              'packet_review.py', 'reference_review.py', 'episodes.py', 'review_packets.py', 'data.py', 'uv.lock')},
              'parameters': {'prior_users': 3, 'raw_control_records': 64, 'visible_control_records': 16,
                             'visible_control_raw_ceiling': 4096, 'message_bytes': 16384, 'display_bytes': 32768},
              'inference_calls': 0,
              'limitations': ['changed raw scope, not interchangeable source coverage or a semantic quality comparison',
                  'stored traversal verified; native audit only current canonical identities, not complete order/tool bodies',
                  'visible cap may stop before three users; byte display limits remain independent of model windows',
                  'capture timing includes source validation only for raw control; not fair kernel speed or ingestion comparison',
                  'small exposed Codex-only reference cohort; no resolution labels or production promotion']}
    with args.output.open('x') as output:
        json.dump(report, output, ensure_ascii=False, indent=2)
    for control in Control:
        contexts = [s for c in cases for s in c['controls'][control]['contexts']]
        print(json.dumps({'control': control, 'anchors': len(contexts), 'stops': Counter(s['stop'] for s in contexts),
                          'prior_users': Counter(sum(e['role'] == Role.USER for e in s['display']) for s in contexts),
                          'raw_records': sum(len(s['traversed']) for s in contexts),
                          'capture_ms': sum(c['controls'][control]['capture_ms'] for c in cases)}))


if __name__ == '__main__':
    main()
