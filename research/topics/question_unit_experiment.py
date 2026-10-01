"""Bounded complete question units, exact vector reuse, and fixed development views."""
import argparse
from dataclasses import asdict
import importlib.metadata
import json
from pathlib import Path
import time

import numpy as np
from tokenizers import Tokenizer

from bounded_question_units import capture_plan, plan
from data import normalized
from external_context import compare
from local_embeddings import Encoder
from question_body_support import BODY_CAPTURE
from question_html import render
from question_representations import read_title_capture
from question_transfer import question_pairs, rank_metrics
from reference_review import digest
from stackoverflow_source import load, split


def frozen_body(path):
    raw = path.read_bytes()
    if digest(raw) != BODY_CAPTURE:
        raise ValueError('original frozen body capture required')
    body = json.loads(raw)
    for name, expected in body['code_pins'].items():
        if digest(Path(__file__).with_name(name).read_bytes()) != expected:
            raise ValueError('original body contract drift')
    for name, expected in body['versions'].items():
        if importlib.metadata.version(name) != expected:
            raise ValueError('original body dependencies drift')
    return body


def experiment(root, snapshot, body_path, title_path):
    started = time.perf_counter()
    body = frozen_body(body_path)
    title = read_title_capture(title_path)
    corpus = load(root)
    cal, former_reserved, _ = split(corpus)
    selected = (*cal, *former_reserved)
    references = {str(r.id): r.tags for r in corpus.references}
    tokenizer_path = snapshot / 'tokenizer.json'
    if digest(tokenizer_path.read_bytes()) != body['encoder']['tokenizer_sha256']:
        raise ValueError('original tokenizer asset required')
    tokenizer = Tokenizer.from_file(str(tokenizer_path))
    tokenizer.no_padding()
    tokenizer.no_truncation()
    before = time.perf_counter()
    plans, rendered, source_inputs = {}, {}, {}
    for q in selected:
        identity = str(q.id)
        original = render(q.body_html)
        if digest(original.text.encode()) != body['records'][identity]['rendered_sha256']:
            raise ValueError('original displayed body drift')
        result = plan(q.title, original, lambda text: len(tokenizer.encode(text).ids))
        plans[identity], rendered[identity] = result, original
        if result.reason is None:
            source_inputs[identity] = [q.title + '\n\n' + original.text[u.start:u.end]
                                       for u in result.units if u.reason is None]
    unique_inputs = {t for values in source_inputs.values() for t in values}
    whole_inputs = {q.title + '\n\n' + rendered[str(q.id)].text: str(q.id) for q in selected
                    if body['support'][str(q.id)]['reason'] is None}
    if len(unique_inputs | set(whole_inputs)) > 1024:
        raise ValueError('bounded inference preflight exceeded; no resampling')
    timings = {'planning_ms': (time.perf_counter() - before) * 1000}
    print(json.dumps({'stage': 'planned', 'questions': len(source_inputs), 'unique_inputs': len(unique_inputs),
                      'cached_whole_inputs': len(whole_inputs)}), flush=True)
    encoder = Encoder(snapshot)
    if (encoder.statistics.model_sha256 != body['encoder']['model_sha256']
            or encoder.statistics.tokenizer_sha256 != body['encoder']['tokenizer_sha256']):
        raise ValueError('original installed model assets required')
    for text, identity in whole_inputs.items():
        if digest(text.encode()) != body['records'][identity]['input_sha256']:
            raise ValueError('cached model input identity drift')
        vector = np.asarray(body['support'][identity]['vector'], dtype=np.float64)
        if vector.shape != (384,) or not np.isfinite(vector).all() or not np.isclose(np.linalg.norm(vector), 1., rtol=1e-12):
            raise ValueError('invalid original cached direction')
        encoder.cache[text] = vector
    vectors, means, unit_vectors = {}, {}, {}
    for n, identity in enumerate(sorted(source_inputs, key=int)):
        inputs = list(dict.fromkeys(source_inputs[identity]))
        matrix = np.asarray([encoder.encode(t) for t in inputs], dtype=np.float64)
        means[identity] = normalized(matrix.mean(axis=0))
        unit_vectors[identity] = matrix
        for text, vector in zip(inputs, matrix):
            vectors[digest(text.encode())] = vector.tolist()
        if (n + 1) % 16 == 0:
            print(json.dumps({'stage': 'encoded', 'questions': n + 1, 'calls': encoder.statistics.calls,
                              'reuse_hits': encoder.statistics.cache_hits}), flush=True)
    phases = {}
    before = time.perf_counter()
    for phase, questions in [('calibration-source', cal), ('reserved-source', former_reserved)]:
        original = title['phases'][phase]
        ids = [i for i in original['fit_identities'] if i in means]
        if len(ids) < 2:
            raise ValueError('insufficient complete question support; no resampling')
        titles = {i: np.asarray(original['records'][i]['vector'], dtype=np.float64) for i in ids}
        sparse = {i: dict(zip(body['lexical_records'][i]['indices'], body['lexical_records'][i]['values'])) for i in ids}
        rows = question_pairs(ids, references)
        scores = {view: [] for view in ('title', 'title75lex25', 'unit-mean', 'unit-max', 'mean75lex25', 'max75lex25')}
        for row in rows:
            a, b = row['left'], row['right']
            lexical = float(np.clip(sum(v * sparse[b].get(k, 0.) for k, v in sparse[a].items()), -1, 1))
            title_score = float(np.clip(titles[a] @ titles[b], -1, 1))
            mean = float(np.clip(means[a] @ means[b], -1, 1))
            maximum = float(np.clip((unit_vectors[a] @ unit_vectors[b].T).max(), -1, 1))
            values = (title_score, .75 * title_score + .25 * lexical, mean, maximum,
                      .75 * mean + .25 * lexical, .75 * maximum + .25 * lexical)
            for view, value in zip(scores, values):
                scores[view].append(value)
        phases[phase] = {'identities': ids, 'rows': rows, 'scores': scores,
                         'metrics': {v: rank_metrics(rows, s) for v, s in scores.items()},
                         'original_title_metrics': original['title_cosine']}
    timings['scoring_ms'] = (time.perf_counter() - before) * 1000
    stats = asdict(encoder.statistics)
    for name in ('inference_ms', 'tokenization_ms'):
        timings[name] = stats.pop(name)
    timings['total_ms'] = (time.perf_counter() - started) * 1000
    return {'version': 1, 'scope': 'exposed-bounded-question-units-no-selection',
            'policy': {'body_sha256': BODY_CAPTURE, 'title_sha256': body['policy']['title_capture_sha256'],
                       'maximum_tokens': 512, 'maximum_units': 32, 'maximum_unique_inputs': 1024,
                       'scoring': 'deduplicate-identical-inputs-per-question', 'dense_weight': .75},
            'code_pins': {n: digest(Path(__file__).with_name(n).read_bytes()) for n in
                          ('question_unit_experiment.py', 'bounded_question_units.py', 'question_html.py',
                           'question_body_support.py', 'question_representations.py', 'question_transfer.py',
                           'stackoverflow_source.py', 'local_embeddings.py', 'data.py', 'reference_review.py', 'uv.lock')},
            'plans': {i: capture_plan(p) for i, p in plans.items()},
            'input_hashes': {i: [digest(t.encode()) for t in values] for i, values in source_inputs.items()},
            'vectors': vectors, 'mean_vectors': {i: m.tolist() for i, m in means.items()},
            'encoder': stats, 'cached_original_inputs': len(whole_inputs),
            'phases': phases, 'timings': timings}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('root', 'snapshot', 'body-capture', 'title-capture', 'output'):
        parser.add_argument(f'--{name}', type=Path, required=True)
    parser.add_argument('--compare', type=Path)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git output required')
    result = experiment(args.root, args.snapshot, args.body_capture, args.title_capture)
    if args.compare:
        compare(json.loads(args.compare.read_text()), result)
    with args.output.open('x') as output:
        json.dump(result, output, sort_keys=True, allow_nan=False)
    print(json.dumps({'stage': 'complete', 'calls': result['encoder']['calls'],
                      'support': {p: len(v['identities']) for p, v in result['phases'].items()}}))


if __name__ == '__main__':
    main()
