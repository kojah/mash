"""Validate complete blind reviews; agreement is not semantic accuracy."""
import argparse
from collections import Counter
from enum import StrEnum
import json
from pathlib import Path

from blind_topic_packet import Stratum, template
from reference_review import digest


class Decision(StrEnum):
    YES = 'yes'
    NO = 'no'
    UNCERTAIN = 'uncertain'


def keys(value, expected):
    if not isinstance(value, dict) or set(value) != set(expected):
        raise ValueError('review schema keys differ')


def text(value):
    if not isinstance(value, str) or not value.strip():
        raise ValueError('nonempty text required')


def rows(value, expected):
    if not isinstance(value, list) or len(value) != len(expected):
        raise ValueError('complete identity coverage required')
    found = {}
    for row in value:
        if not isinstance(row, dict) or not isinstance(row.get('id'), str) or row['id'] in found:
            raise ValueError('unique row identities required')
        found[row['id']] = row
    if set(found) != set(expected):
        raise ValueError('review identities differ from packet')
    return found


def evidence(value, sources, allowed):
    if not isinstance(value, list):
        raise ValueError('evidence array required')
    covered = set()
    for item in value:
        keys(item, ('source_id', 'field', 'quote'))
        source, field, quote = item['source_id'], item['field'], item['quote']
        if not isinstance(source, str) or source not in allowed or field not in ('title', 'body_displayed', 'body_html'):
            raise ValueError('evidence source/field outside review scope')
        text(quote)
        if quote not in sources[source][field]:
            raise ValueError('evidence must be an exact source quote')
        covered.add(source)
    return covered


def validate(packet_bytes, answer):
    packet = decode(packet_bytes)
    pin = digest(packet_bytes)
    expected = template(packet, pin)
    keys(answer, expected)
    if (type(answer['version']) is not int or answer['version'] != 1
            or answer['scope'] != expected['scope'] or answer['packet_sha256'] != pin):
        raise ValueError('review packet/version mismatch')
    reviewer = answer['reviewer']
    keys(reviewer, expected['reviewer'])
    text(reviewer['identity'])
    for key in ('human_attestation', 'case_reference_exposure', 'case_prediction_exposure'):
        if type(reviewer[key]) is not bool:
            raise ValueError('explicit literal reviewer declarations required')
    if not reviewer['human_attestation']:
        raise ValueError('human review attestation required; not automatically verified')
    sources = {d['id']: d for d in packet['documents']}
    documents = rows(answer['documents'], sources)
    for source_id, row in documents.items():
        keys(row, expected['documents'][0])
        if type(row['sufficient_context']) is not bool:
            raise ValueError('context decision required')
        if row['primary_subject'] is not None:
            text(row['primary_subject'])
        elif row['sufficient_context']:
            raise ValueError('primary subject required with sufficient context')
        secondary = row['secondary_subjects']
        if not isinstance(secondary, list):
            raise ValueError('explicit secondary subject list required')
        for subject in secondary:
            text(subject)
        if len(set(secondary)) != len(secondary):
            raise ValueError('duplicate secondary subjects')
        if evidence(row['source_evidence'], sources, {source_id}) != {source_id}:
            raise ValueError('source-grounded document evidence required')
        if row['notes'] is not None:
            text(row['notes'])
    cases = {c['id']: c for c in packet['cases']}
    judgments = rows(answer['cases'], cases)
    for case_id, row in judgments.items():
        keys(row, expected['cases'][0])
        endpoints = {cases[case_id]['left'], cases[case_id]['right']}
        for decision, subject in (('shared_specific_subject', 'specific_subject'),
                                  ('useful_broader_subject', 'broader_subject')):
            if row[decision] not in tuple(Decision):
                raise ValueError('explicit yes/no/uncertain decision required')
            if any(not documents[i]['sufficient_context'] for i in endpoints) and row[decision] != Decision.UNCERTAIN:
                raise ValueError('insufficient endpoint context requires uncertainty')
            if row[decision] == Decision.YES:
                text(row[subject])
            elif row[subject] is not None:
                raise ValueError('nonpositive common subject must be null')
        text(row['rationale'])
        if evidence(row['source_evidence'], sources, endpoints) != endpoints:
            raise ValueError('pair evidence must cover both sources')
    return answer


def agreement(packet_bytes, first, second, manifest):
    a, b = validate(packet_bytes, first), validate(packet_bytes, second)
    if a['reviewer']['identity'] == b['reviewer']['identity']:
        raise ValueError('distinct declared reviewer identities required')
    if manifest['packet_sha256'] != digest(packet_bytes):
        raise ValueError('selection manifest packet mismatch')
    left = {r['id']: r for r in a['cases']}
    right = {r['id']: r for r in b['cases']}
    strata = {}
    for row in manifest['selection']:
        if row['case'] in strata or row['stratum'] not in tuple(Stratum):
            raise ValueError('unique known selection strata required')
        strata[row['case']] = row['stratum']
    if set(strata) != set(left):
        raise ValueError('complete manifest coverage required')
    result = {}
    for stratum in Stratum:
        ids = sorted(i for i in left if strata[i] == stratum)
        result[stratum] = {}
        for field in ('shared_specific_subject', 'useful_broader_subject'):
            matrix = Counter((left[i][field], right[i][field]) for i in ids)
            n = len(ids)
            matches = sum(matrix[d, d] for d in Decision)
            observed = matches / n if n else None
            expected = sum(sum(matrix[d, e] for e in Decision) * sum(matrix[e, d] for e in Decision)
                           for d in Decision) / (n * n) if n else None
            result[stratum][field] = {'count': n, 'matches': matches, 'agreement': observed,
                'kappa': (observed - expected) / (1 - expected) if expected is not None and expected < 1 else None,
                'matrix': [[matrix[d, e] for e in Decision] for d in Decision],
                'uncertain_either': sum(left[i][field] == Decision.UNCERTAIN or right[i][field] == Decision.UNCERTAIN for i in ids)}
    return {'version': 1, 'scope': 'declared-human-pair-agreement-not-accuracy',
            'packet_sha256': digest(packet_bytes), 'categories': list(Decision),
            'reviewer_declarations': [a['reviewer'], b['reviewer']], 'strata': result}


def decode(raw):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError('duplicate JSON field')
            result[key] = value
        return result
    def invalid(value):
        raise ValueError('nonfinite JSON value')
    return json.loads(raw, object_pairs_hook=unique, parse_constant=invalid)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--packet', type=Path, required=True)
    parser.add_argument('--answers', type=Path, required=True)
    parser.add_argument('--second', type=Path)
    parser.add_argument('--manifest', type=Path)
    args = parser.parse_args()
    raw = args.packet.read_bytes()
    first = validate(raw, decode(args.answers.read_bytes()))
    if bool(args.second) != bool(args.manifest):
        raise ValueError('second review and selection manifest must be supplied together')
    result = agreement(raw, first, decode(args.second.read_bytes()), decode(args.manifest.read_bytes())) if args.second else {
        'scope': 'complete-declared-human-review-validated-not-independent-gold', 'packet_sha256': digest(raw)}
    print(json.dumps(result, allow_nan=False))


if __name__ == '__main__':
    main()
