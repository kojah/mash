"""Source-grounded body/lexical controls on exposed programming development data."""
import argparse
from dataclasses import asdict
from enum import StrEnum
import importlib.metadata
import json
from pathlib import Path
import time

import numpy as np
from scipy.sparse import vstack

from cluster_calibration import MODEL, TOKENIZER
from external_context import compare
from lexical_context import lexical_features, serialized_policy
from local_embeddings import Encoder
from question_html import InvalidHtml, render
from question_transfer import question_pairs, rank_metrics
from reference_review import digest
from stackoverflow_source import PINS, load, split


TITLE_CAPTURE = 'a77d5efeb2ce4a816419bf6ed3efc1c5bb0dfcde26988c91c6280293db619333'


class View(StrEnum):
    TITLE = 'title'
    JOINED = 'title-body'
    LEXICAL = 'lexical-full-question'
    TITLE_FUSION = 'title-75-lexical-25'
    JOINED_FUSION = 'joined-75-lexical-25'


def read_title_capture(path):
    raw = path.read_bytes()
    if digest(raw) != TITLE_CAPTURE:
        raise ValueError('frozen title capture required')
    capture = json.loads(raw)
    for name, expected in capture['code_pins'].items():
        if digest(Path(__file__).with_name(name).read_bytes()) != expected:
            raise ValueError('original title code contract drift')
    for name, expected in capture['versions'].items():
        if importlib.metadata.version(name) != expected:
            raise ValueError('original title dependency drift')
    if capture['policy']['source_pins'] != PINS:
        raise ValueError('original title source contract drift')
    return capture


def source_inputs(questions):
    texts, records = {}, {}
    for q in questions:
        identity = str(q.id)
        try:
            rendered = render(q.body_html)
        except InvalidHtml as error:
            records[identity] = {'reason': str(error), 'group': q.group, 'source_sha256': q.source_sha256}
            continue
        records[identity] = {'reason': None if rendered.text.strip() else 'empty-rendered-body',
                             'group': q.group, 'source_sha256': q.source_sha256,
                             'html_sha256': rendered.source_sha256, 'rendered_sha256': digest(rendered.text.encode()),
                             'rendered_bytes': len(rendered.text.encode()), 'pieces': [asdict(p) for p in rendered.pieces]}
        if rendered.text.strip():
            texts[identity] = q.title + '\n\n' + rendered.text
            records[identity]['input_sha256'] = digest(texts[identity].encode())
    return texts, records


def pair_scores(rows, identities, titles, joined, features):
    positions = {i: n for n, i in enumerate(identities)}
    lexical = (features @ features.T).toarray()
    scores = {view: [] for view in View}
    for row in rows:
        a, b = row['left'], row['right']
        title = float(np.clip(titles[a] @ titles[b], -1, 1))
        body = float(np.clip(joined[a] @ joined[b], -1, 1))
        sparse = float(np.clip(lexical[positions[a], positions[b]], -1, 1))
        values = {View.TITLE: title, View.JOINED: body, View.LEXICAL: sparse,
                  View.TITLE_FUSION: .75 * title + .25 * sparse,
                  View.JOINED_FUSION: .75 * body + .25 * sparse}
        for view, value in values.items():
            scores[view].append(value)
    return scores


def experiment(root, snapshot, title_path):
    start = time.perf_counter()
    title_capture = read_title_capture(title_path)
    corpus = load(root)
    cal, former_reserved, background = split(corpus)
    references = {str(r.id): r.tags for r in corpus.references}
    before = time.perf_counter()
    texts, records = source_inputs(corpus.inputs)
    selected = {str(q.id) for q in (*cal, *former_reserved)}
    forbidden = {texts[i] for i in selected if i in texts}
    source_background = [texts[str(q.id)] for q in background if str(q.id) in texts]
    allowed = [t for t in source_background if t not in forbidden]
    background_texts = sorted(set(allowed), key=lambda t: (digest(t.encode()), t))
    timings = {'render_ms': (time.perf_counter() - before) * 1000}
    encoder = Encoder(snapshot)
    if encoder.statistics.model_sha256 != MODEL or encoder.statistics.tokenizer_sha256 != TOKENIZER:
        raise ValueError('original pinned installed assets required')
    eligible = [i for i in selected if i in texts]
    if len(set(texts[i] for i in eligible)) > 128:
        raise ValueError('128 unique body input preflight cap')
    before = time.perf_counter()
    vectorizer, lexical, background_nnz = lexical_features(background_texts, [texts[i] for i in sorted(eligible)])
    features = {i: lexical[n] for n, i in enumerate(sorted(eligible))}
    timings['lexical_ms'] = (time.perf_counter() - before) * 1000
    joined, support = {}, {}
    cal_inputs = {texts[str(q.id)] for q in cal if str(q.id) in texts}
    for qs, phase in [(cal, 'calibration-source'), (former_reserved, 'reserved-source')]:
        for q in sorted(qs, key=lambda q: q.id):
            i = str(q.id)
            if i not in texts:
                support[i] = {'reason': records[i]['reason'], 'tokens': None, 'vector': None}
                continue
            tokens = len(encoder.tokenizer.encode(texts[i]).ids)
            reason = ('exact-development-input' if phase == 'reserved-source' and texts[i] in cal_inputs
                      else 'model-window' if not 2 <= tokens <= 512 else None)
            vector = encoder.encode(texts[i]) if reason is None else None
            support[i] = {'reason': reason, 'tokens': tokens, 'vector': vector.tolist() if vector is not None else None,
                          'lexical_zero': features[i].nnz == 0}
            if vector is not None:
                joined[i] = vector
    phases = {}
    for phase, questions in [('calibration-source', cal), ('reserved-source', former_reserved)]:
        original = title_capture['phases'][phase]
        ids = original['fit_identities']
        if set(ids) != {str(q.id) for q in questions}:
            raise ValueError('original title support drift')
        title_vectors = {i: np.asarray(original['records'][i]['vector'], dtype=np.float64) for i in ids}
        for q in questions:
            record = original['records'][str(q.id)]
            if record['group'] != q.group or record['source_sha256'] != q.source_sha256 or record['title_sha256'] != digest(q.title.encode()):
                raise ValueError('original title identity drift')
        original_rows = question_pairs(ids, references)
        original_scores = [float(np.clip(title_vectors[r['left']] @ title_vectors[r['right']], -1, 1)) for r in original_rows]
        if original_rows != original['rows'] or original_scores != original['title_cosine_scores'] or rank_metrics(original_rows, original_scores) != original['title_cosine']:
            raise ValueError('original title baseline drift')
        common = [i for i in ids if i in joined and i in features and features[i].nnz]
        if len(common) < 2:
            raise ValueError('insufficient common support; do not resample')
        rows = question_pairs(common, references)
        scores = pair_scores(rows, common, title_vectors, joined, vstack([features[i] for i in common]))
        phases[phase] = {'original_title_metrics': original['title_cosine'], 'fit_identities': common, 'rows': rows,
                         'scores': scores, 'metrics': {view: rank_metrics(rows, values) for view, values in scores.items()}}
    stats = asdict(encoder.statistics)
    for name in ('inference_ms', 'tokenization_ms'):
        timings[name] = stats.pop(name)
    timings['total_ms'] = (time.perf_counter() - start) * 1000
    return {'version': 1, 'scope': 'exposed-programming-body-development-no-selection',
            'policy': {'source_pins': PINS, 'title_capture_sha256': TITLE_CAPTURE, 'lexical': serialized_policy(),
                       'dense_weight': .75, 'html_segment_cap': 4096, 'rendered_byte_cap': 65536, 'unique_body_input_cap': 128},
            'code_pins': {n: digest(Path(__file__).with_name(n).read_bytes()) for n in
                          ('question_representations.py', 'question_html.py', 'question_transfer.py', 'stackoverflow_source.py',
                           'lexical_context.py', 'local_embeddings.py', 'data.py', 'external_context.py', 'reference_review.py', 'uv.lock')},
            'encoder': stats, 'versions': {n: importlib.metadata.version(n) for n in
                                          ('numpy', 'scipy', 'scikit-learn', 'onnxruntime', 'tokenizers')},
            'records': records, 'support': support,
            'background': {'groups': [q.group for q in background], 'source_inputs': len(source_background),
                           'exact_selected_input_exclusions': len(source_background) - len(allowed),
                           'duplicate_exclusions': len(allowed) - len(background_texts),
                           'input_hashes': [digest(t.encode()) for t in background_texts], 'nnz': background_nnz,
                           'vocabulary': vectorizer.get_feature_names_out().tolist(), 'idf': vectorizer.idf_.tolist()},
            'lexical_records': {i: {'indices': features[i].indices.tolist(), 'values': features[i].data.tolist()} for i in features},
            'phases': phases, 'timings': timings}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('root', 'snapshot', 'title-capture', 'output'):
        parser.add_argument(f'--{name}', type=Path, required=True)
    parser.add_argument('--compare', type=Path)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git output required')
    result = experiment(args.root, args.snapshot, args.title_capture)
    if args.compare:
        compare(json.loads(args.compare.read_text()), result)
    with args.output.open('x') as output:
        json.dump(result, output, sort_keys=True, allow_nan=False)
    print(json.dumps({'calls': result['encoder']['calls'], 'support': {p: len(v['fit_identities']) for p, v in result['phases'].items()}}))


if __name__ == '__main__':
    main()
