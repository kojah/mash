"""Frozen annotation-free lexical background and dense/sparse cosine controls."""
import argparse
from collections import Counter
from enum import StrEnum
import importlib.metadata
import json
from pathlib import Path
import time

import numpy as np
from sklearn.feature_extraction.text import TfidfVectorizer

from external_analysis import CAPTURE, prepare, evaluation_grid, summarize, weighted_metrics
from external_context import Control, compare
from multiwoz import Speaker, load
from multiview import ANALYSIS
from reference_review import digest


PARAMETERS = {'analyzer': 'word', 'ngram_range': (1, 2), 'token_pattern': r'(?u)\b\w\w+\b',
              'lowercase': True, 'min_df': 2, 'max_features': 8192, 'smooth_idf': True,
              'sublinear_tf': True, 'norm': 'l2', 'stop_words': None}


class LexicalControl(StrEnum):
    LEXICAL = 'lexical'
    CURRENT = 'current_75_lexical_25'
    JOINED = 'joined_75_lexical_25'


def serialized_policy():
    # Library parameters use a tuple; the stage contract uses a JSON array.
    return {**PARAMETERS, 'ngram_range': list(PARAMETERS['ngram_range'])}


def background(dialogues, selected_groups):
    """Roles and original text only. Exclude full groups and exact evaluation text."""
    all_groups = {d.group for d in dialogues}
    if not selected_groups or not selected_groups < all_groups:
        raise ValueError('disjoint nonempty background and evaluation groups required')
    evaluation = {t.text for d in dialogues if d.group in selected_groups for t in d.turns if t.speaker == Speaker.USER}
    source = [t.text for d in dialogues if d.group not in selected_groups for t in d.turns if t.speaker == Speaker.USER]
    allowed = [text for text in source if text not in evaluation]
    texts = sorted(set(allowed), key=lambda t: (digest(t.encode()), t))
    if not 2 <= len(texts) <= 4096:
        raise ValueError('bounded disjoint lexical background required')
    return texts, {'groups': sorted(all_groups - selected_groups), 'source_user_turns': len(source),
                   'exact_evaluation_text_exclusions': len(source) - len(allowed),
                   'duplicate_background_text_exclusions': len(allowed) - len(texts),
                   'unique_text_hashes': [digest(t.encode()) for t in texts]}


def lexical_features(background_texts, original_texts):
    if not 2 <= len(background_texts) <= 4096 or not 1 <= len(original_texts) <= 1024 or any(not isinstance(t, str) or not t.strip() or len(t.encode()) > 65536
                                                       for t in (*background_texts, *original_texts)):
        raise ValueError('exact bounded nonempty source text required')
    if set(background_texts) & set(original_texts):
        raise ValueError('exact evaluation text leaked into background fit')
    vectorizer = TfidfVectorizer(**PARAMETERS, dtype=np.float64)
    fitted = vectorizer.fit_transform(background_texts)
    features = vectorizer.transform(original_texts)
    return vectorizer, features, fitted.nnz


def fuse(current, joined, lexical):
    if any(not np.isfinite(s) or not -1.000000000001 <= s <= 1.000000000001 for s in (current, joined, lexical)):
        raise ValueError('finite normalized cosine scores required')
    return {LexicalControl.LEXICAL: lexical, LexicalControl.CURRENT: 0.75 * current + 0.25 * lexical,
            LexicalControl.JOINED: 0.75 * joined + 0.25 * lexical}


def experiment(root, capture_path, analysis_path):
    started = time.perf_counter()
    raw, raw_analysis = capture_path.read_bytes(), analysis_path.read_bytes()
    if digest(raw) != CAPTURE or digest(raw_analysis) != ANALYSIS:
        raise ValueError('frozen vector or bootstrap input drift')
    capture, archived = json.loads(raw), json.loads(raw_analysis)
    versions = {name: importlib.metadata.version(name) for name in ('numpy', 'scikit-learn')}
    if versions != archived['versions']:
        raise ValueError('frozen evaluation dependency versions changed')
    corpus = load(root)
    anchors, vectors, common, groups = prepare(corpus, capture)
    original_rows = evaluation_grid(anchors, vectors, common)
    if original_rows != archived['common_pairs'] or anchors != archived['anchors']:
        raise ValueError('original complete pair grid or annotations changed')
    for name, gold_key, rows in (('service', 'service_overlap', original_rows), ('intent', 'intent_overlap', original_rows),
                                ('within_service_intent', 'intent_overlap', [r for r in original_rows if r['service_overlap']])):
        for control in Control:
            if weighted_metrics([r[gold_key] for r in rows], [r['scores'][control] for r in rows], np.ones(len(rows))) != archived['slices'][name]['metrics'][control]:
                raise ValueError('original baseline metric drift')
    feature_start = time.perf_counter()
    background_texts, provenance = background(corpus.dialogues, set(groups))
    turns = {f'{t.dialogue}:{t.ordinal}': t.text for d in corpus.dialogues for t in d.turns if t.speaker == Speaker.USER}
    identities = sorted(common)
    vectorizer, features, background_nnz = lexical_features(background_texts, [turns[key] for key in identities])
    positions = {key: index for index, key in enumerate(identities)}
    eligible = {key for key in identities if features[positions[key]].nnz}
    feature_ms = (time.perf_counter() - feature_start) * 1000
    score_start = time.perf_counter()
    lexical_cosines = (features @ features.T).toarray()
    rows = [r for r in original_rows if r['left'] in eligible and r['right'] in eligible]
    for row in rows:
        row['scores'].update(fuse(row['scores']['current'], row['scores']['joined'],
                                 float(lexical_cosines[positions[row['left']], positions[row['right']]])))
    score_ms = (time.perf_counter() - score_start) * 1000
    controls = (*Control, *LexicalControl)
    draws = archived['draws']
    if len(draws) != 1000 or any(set(d) != set(groups) or sum(d.values()) != 32 for d in draws):
        raise ValueError('archived resampling coverage changed')
    slices = {'service': summarize(rows, 'service_overlap', draws, controls),
              'intent': summarize(rows, 'intent_overlap', draws, controls),
              'within_service_intent': summarize([r for r in rows if r['service_overlap']], 'intent_overlap', draws, controls)}
    domain_results = {}
    for service in sorted({s for a in anchors.values() if not a['unknown'] for s in a['active']}):
        selected = [r for r in rows if service in r['services']]
        gold = [service in r['shared_services'] for r in selected]
        domain_results[service] = {'pairs': len(selected), 'positive': sum(gold), 'metrics': {
            c: weighted_metrics(gold, [r['scores'][c] for r in selected], np.ones(len(selected))) for c in controls}}
    records = {}
    for key in identities:
        feature = features[positions[key]]
        records[key] = {'text_sha256': digest(turns[key].encode()), 'reason': None if key in eligible else 'zero-lexical',
                        'indices': feature.indices.tolist(), 'values': feature.data.tolist()}
    code = {name: digest(Path(__file__).with_name(name).read_bytes()) for name in
            ('lexical_context.py', 'external_analysis.py', 'external_context.py', 'multiwoz.py')}
    return {'version': 1, 'vector_capture_sha256': CAPTURE, 'bootstrap_capture_sha256': ANALYSIS,
            'source_pins': capture['source_pins'], 'code_pins': code, 'versions': versions, 'policy': serialized_policy(),
            'dense_weight': 0.75, 'background': provenance, 'background_nnz': background_nnz,
            'vocabulary': vectorizer.get_feature_names_out().tolist(), 'idf': vectorizer.idf_.tolist(),
            'selected_groups': capture['selected_groups'], 'anchors': anchors, 'draws': draws,
            'original_model_support': len(common), 'matched_support': len(eligible),
            'abstentions': dict(Counter(r['reason'] for r in records.values() if r['reason'])),
            'original_pair_count': len(original_rows), 'matched_pair_count': len(rows),
            'records': records, 'common_pairs': rows, 'slices': slices, 'domains': domain_results, 'new_inference_calls': 0,
            'timings': {'features_ms': feature_ms, 'candidate_scores_ms': score_ms,
                        'total_after_imports_ms': (time.perf_counter() - started) * 1000}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--capture', type=Path, required=True)
    parser.add_argument('--analysis', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--compare', type=Path)
    args = parser.parse_args()
    result = experiment(args.root, args.capture, args.analysis)
    if args.compare:
        compare(json.loads(args.compare.read_text()), result)
    with args.output.open('x') as destination:
        json.dump(result, destination, sort_keys=True, allow_nan=False)
    print(json.dumps({name: {key: value for key, value in s.items() if key != 'paired_replicate_deltas'}
                      for name, s in result['slices'].items()}))


if __name__ == '__main__':
    main()
