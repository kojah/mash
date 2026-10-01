"""Validate source-backed judgments for the fixed visible-budget development control."""
import argparse
from collections import Counter
import json
from pathlib import Path

from dialogue_budget import Control
from packet_review import Origin, validate as validate_origins
from reference_review import digest, validate_anchors


def validate(context_bytes, packet_bytes, review_bytes, annotation):
    judgments = validate_origins(packet_bytes, review_bytes)
    context = json.loads(context_bytes)
    if (context.get('schema_version') != 1 or context.get('scope') != 'development-visible-dialogue-budget-control'
            or context['input_sha256']['packets'] != digest(packet_bytes)
            or context['input_sha256']['review'] != digest(review_bytes)):
        raise ValueError('visible reference context/source changed')
    review = json.loads(review_bytes)
    rows = {(c['thread'], s['ordinal']): (c['source_sha256'], s) for c in review['cases'] for s in c['turns']}
    expected = {}
    for j in judgments:
        if j.origin == Origin.REFERENCE:
            source, item = rows[j.thread, j.ordinal]
            expected[j.thread, j.ordinal] = (source, {'message': item['id'], 'text_sha256': item['text_sha256']})
    cases = []
    for case in context['cases']:
        if set(case['controls']) != set(Control):
            raise ValueError('incomplete context control grid')
        contexts = case['controls'][Control.VISIBLE]['contexts']
        if any(s.get('maximum_visible') != 16 or s.get('maximum_users') != 3
               or s['bounds'] != [4096, 16384, 32768] for s in contexts):
            raise ValueError('visible reference context bounds changed')
        cases.append({'thread': case['thread'], 'source_sha256': case['source_sha256'], 'contexts': contexts})
    return validate_anchors(context_bytes, review_bytes, annotation, expected, cases)


def availability(context_bytes, annotation):
    """Oracle evidence availability only; call after complete source validation."""
    context = json.loads(context_bytes)
    judgments = {(j['thread'], j['ordinal']): j for j in annotation['judgments']}
    result = {}
    for control in Control:
        resolved, available, displayed, size = 0, 0, 0, 0
        for case in context['cases']:
            for captured in case['controls'][control]['contexts']:
                displayed += len(captured['display'])
                size += captured['display_bytes']
                judgment = judgments[case['thread'], captured['anchor']['ordinal']]
                if judgment['resolution'] != 'resolved-task':
                    continue
                resolved += 1
                entries = {e['ordinal']: e for e in captured['display'] if e['display_status'] == 'included'}
                available += all(e['ordinal'] in entries
                    and all(entries[e['ordinal']][k] == v for k, v in e.items())
                    and digest(entries[e['ordinal']]['text'].encode()) == e['searchable_sha256']
                    for e in judgment['evidence'])
        result[control] = {'reviewed_task_references': resolved, 'all_cited_evidence_available': available,
                           'display_records': displayed, 'display_bytes': size}
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('contexts', 'packets', 'source-review', 'annotations'):
        parser.add_argument(f'--{name}', type=Path, required=True)
    args = parser.parse_args()
    contexts, annotation = args.contexts.read_bytes(), json.loads(args.annotations.read_text())
    result = validate(contexts, args.packets.read_bytes(), args.source_review.read_bytes(), annotation)
    print(json.dumps({'judgments': len(result), 'resolutions': Counter(j.resolution for j in result),
                      'oracle_evidence_availability': availability(contexts, annotation)}))


if __name__ == '__main__':
    main()
