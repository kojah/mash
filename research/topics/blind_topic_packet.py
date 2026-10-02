"""Source-only pilot review bundle with selection/reference data kept separate."""
import argparse
from dataclasses import dataclass
from enum import StrEnum
import html
import json
from pathlib import Path
import time

from external_context import compare
from question_html import render
from reference_review import digest
from stackoverflow_source import encoded, load


SEED = 'mash-topic-blind-pilot-v1-20261002'
INPUT_PIN = '95d1bbbb298887322fc6de3566ff10328e9b5b33110748b3f22256a1a20a55ad'
MODEL = 'title-prose-code/p05/hard/0.20/3/17'


class Stratum(StrEnum):
    SOURCE = 'outcome-independent-source-sample'
    POSITIVE = 'model-positive-tag-negative-diagnostic'
    NEGATIVE = 'model-negative-tag-positive-diagnostic'


@dataclass(frozen=True)
class Pair:
    left: str
    right: str
    tag_overlap: bool
    prediction: bool


def identity(parts, prefix):
    return prefix + '-' + digest(encoded(parts))[:24]


def select(pairs, groups, seed=SEED, primary=16, diagnostic=8):
    if (not isinstance(seed, str) or not seed.strip() or type(primary) is not int
            or type(diagnostic) is not int or not 1 <= primary <= 64 or not 0 <= diagnostic <= 32):
        raise ValueError('nonempty seed and bounded review counts required')
    if len(set(groups.values())) != len(groups):
        raise ValueError('unique canonical source groups required')
    seen = set()
    for pair in pairs:
        key = tuple(sorted((pair.left, pair.right)))
        if (pair.left == pair.right or key in seen or pair.left not in groups or pair.right not in groups
                or type(pair.tag_overlap) is not bool or type(pair.prediction) is not bool):
            raise ValueError('unique nonself source pairs and literal boolean outcomes required')
        seen.add(key)
    def rank(pair):
        return digest(encoded([seed, sorted((groups[pair.left], groups[pair.right]))]))
    ordered = sorted(pairs, key=rank)
    selected = [(p, Stratum.SOURCE) for p in ordered[:primary]]
    chosen = {tuple(sorted((p.left, p.right))) for p, _ in selected}
    for stratum, predicate in ((Stratum.POSITIVE, lambda p: p.prediction and not p.tag_overlap),
                               (Stratum.NEGATIVE, lambda p: p.tag_overlap and not p.prediction)):
        bucket = [p for p in ordered if tuple(sorted((p.left, p.right))) not in chosen and predicate(p)]
        for pair in bucket[:diagnostic]:
            selected.append((pair, stratum))
            chosen.add(tuple(sorted((pair.left, pair.right))))
    return tuple(selected)


def blind(selected, sources, seed=SEED):
    documents, cases, manifest = {}, [], []
    for pair, stratum in selected:
        endpoints = []
        for original in (pair.left, pair.right):
            source = sources[original]
            opaque = identity(['source-v1', source['group']], 'source')
            document = {'id': opaque, 'title': source['title'], 'body_displayed': source['body_displayed'],
                        'body_html': source['body_html']}
            if opaque in documents and documents[opaque] != document:
                raise ValueError('opaque source collision or source drift')
            documents[opaque] = document
            endpoints.append(opaque)
        canonical = sorted(endpoints)
        case_id = identity(['case-v1', canonical], 'case')
        if int(digest(encoded([seed, 'orientation', canonical]))[0], 16) % 2:
            endpoints.reverse()
        cases.append({'id': case_id, 'left': endpoints[0], 'right': endpoints[1]})
        manifest.append({'case': case_id, 'stratum': stratum, 'left_original': pair.left,
                         'right_original': pair.right, 'tag_overlap': pair.tag_overlap, 'prediction': pair.prediction})
    if len({case['id'] for case in cases}) != len(cases):
        raise ValueError('duplicate blind case')
    cases.sort(key=lambda case: digest(encoded([seed, 'display-order', case['id']])))
    packet = {'version': 1, 'scope': 'blind-programming-topic-pilot', 'rubric_version': 1,
              'documents': [documents[key] for key in sorted(documents)], 'cases': cases}
    return packet, sorted(manifest, key=lambda row: row['case'])


def template(packet, packet_sha):
    return {'version': 1, 'scope': packet['scope'], 'packet_sha256': packet_sha,
            'reviewer': {'identity': None, 'human_attestation': None, 'case_reference_exposure': None,
                         'case_prediction_exposure': None},
            'documents': [{'id': d['id'], 'primary_subject': None, 'secondary_subjects': None,
                           'sufficient_context': None, 'source_evidence': None, 'notes': None} for d in packet['documents']],
            'cases': [{'id': c['id'], 'shared_specific_subject': None, 'useful_broader_subject': None,
                       'specific_subject': None, 'broader_subject': None, 'source_evidence': None,
                       'rationale': None} for c in packet['cases']]}


def page(packet, rubric):
    documents = {d['id']: d for d in packet['documents']}
    cards = []
    for index, case in enumerate(packet['cases'], 1):
        source_cards = []
        for label, key in (('A', 'left'), ('B', 'right')):
            document = documents[case[key]]
            source_cards.append('<section><h3>Source ' + label + ': ' + html.escape(document['id']) + '</h3><h4>'
                + html.escape(document['title']) + '</h4><div class="source">' + html.escape(document['body_displayed'])
                + '</div><details><summary>Original captured HTML, escaped</summary><div class="source">'
                + html.escape(document['body_html']) + '</div></details></section>')
        cards.append('<article><h2>Case ' + str(index) + ': ' + html.escape(case['id'])
                     + '</h2><div class="pair">' + ''.join(source_cards) + '</div></article>')
    return ('<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width">'
            '<meta http-equiv="Content-Security-Policy" content="default-src &#39;none&#39;; style-src &#39;unsafe-inline&#39;">'
            '<title>Blind topic review pilot</title><style>body{font:16px/1.5 system-ui;max-width:1500px;margin:2rem auto;padding:1rem}'
            '.pair{display:grid;gap:2rem}section{min-width:0}.source{white-space:pre-wrap;overflow-wrap:anywhere;font-family:monospace}'
            'article{border-top:1px solid #aaa;margin-top:3rem}details{margin-top:1rem}@media(min-width:950px){.pair{grid-template-columns:1fr 1fr}}</style>'
            '<h1>Blind topic review pilot</h1><div class="source">' + html.escape(rubric)
            + '</div>' + ''.join(cards) + '</html>')


def build(root, rubric):
    started = time.perf_counter()
    raw = (root / 'weighted-first.json').read_bytes()
    if digest(raw) != INPUT_PIN:
        raise ValueError('original weighted calibration capture required')
    result = json.loads(raw)
    for name, pin in result['code_pins'].items():
        if digest(Path(__file__).with_name(name).read_bytes()) != pin:
            raise ValueError('original source/method contract changed')
    corpus = load(root)
    questions = {str(q.id): q for q in corpus.inputs}
    references = {str(r.id): r for r in corpus.references}
    predictions = result['results'][MODEL]['predictions']
    pairs = []
    for row, predicted in zip(result['rows'], predictions, strict=True):
        truth = bool(set(references[row['left']].tags) & set(references[row['right']].tags))
        if truth != row['tag_overlap']:
            raise ValueError('original tag proxy changed')
        pairs.append(Pair(row['left'], row['right'], truth, predicted))
    groups = {i: questions[i].group for i in result['identities']}
    selected = select(pairs, groups)
    sources = {i: {'group': q.group, 'title': q.title, 'body_displayed': render(q.body_html).text,
                   'body_html': q.body_html} for i, q in questions.items() if i in groups}
    packet, selection = blind(selected, sources)
    packet_bytes = encoded(packet)
    files = {'review.json': packet_bytes, 'review.html': page(packet, rubric).encode(),
             'answers.json': encoded(template(packet, digest(packet_bytes)))}
    manifest = {'version': 1, 'scope': 'unblinded-pilot-selection-do-not-give-to-reviewers',
                'input_sha256': INPUT_PIN, 'packet_sha256': digest(packet_bytes), 'seed': SEED, 'model': MODEL,
                'requested_counts': {Stratum.SOURCE: 16, Stratum.POSITIVE: 8, Stratum.NEGATIVE: 8},
                'actual_counts': {s: sum(row['stratum'] == s for row in selection) for s in Stratum},
                'selection': selection, 'sources': {i: {'group': groups[i], 'source_sha256': questions[i].source_sha256,
                    'body_html_sha256': digest(questions[i].body_html.encode()), 'tags': list(references[i].tags),
                    'attribution_link': references[i].link, 'content_license': references[i].content_license}
                    for i in sorted({p.left for p, _ in selected} | {p.right for p, _ in selected})}}
    files['unblinded.json'] = encoded(manifest)
    provenance = {'version': 1, 'scope': 'blind-pilot-preparation-no-human-judgments',
                  'files': {name: digest(value) for name, value in files.items()},
                  'case_count': len(packet['cases']), 'document_count': len(packet['documents']),
                  'annotations_received': 0, 'inference_calls': 0, 'rubric_sha256': digest(rubric.encode()),
                  'code_pins': {name: digest(Path(__file__).with_name(name).read_bytes()) for name in
                                ('blind_topic_packet.py', 'question_html.py', 'stackoverflow_source.py', 'uv.lock')},
                  'timings': {'preparation_ms': (time.perf_counter() - started) * 1000}}
    files['provenance.json'] = encoded(provenance)
    return files, provenance


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--output-directory', type=Path, required=True)
    parser.add_argument('--compare', type=Path)
    args = parser.parse_args()
    if args.output_directory.exists() or args.output_directory.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git review directory required')
    rubric = Path(__file__).with_name('blind-topic-review-rubric.md').read_text()
    files, provenance = build(args.root, rubric)
    if args.compare:
        for name, value in files.items():
            previous = (args.compare / name).read_bytes()
            if name == 'provenance.json':
                compare(json.loads(previous), json.loads(value))
            elif previous != value:
                raise ValueError('blind review bundle changed')
    args.output_directory.mkdir()
    for name, value in files.items():
        with (args.output_directory / name).open('xb') as output:
            output.write(value)
    print(json.dumps({'stage': 'complete', 'cases': provenance['case_count'], 'documents': provenance['document_count'],
                      'annotations_received': 0, 'timings': provenance['timings']}))


if __name__ == '__main__':
    main()
