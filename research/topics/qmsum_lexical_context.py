"""Complete source lexical context diagnostic; annotations only define evaluation."""
import argparse
from collections import Counter
from dataclasses import asdict
import hashlib
import json
from pathlib import Path
import platform
import re
import resource
import time

import numpy as np
from scipy import sparse
import sklearn
from sklearn.feature_extraction.text import CountVectorizer, TfidfTransformer

from external_context import compare
from causal_lexical_context import aggregate, contexts, rank
from qmsum import load, membership


TOKEN = re.compile(r'(?u)\b\w\w+\b')
PREFLIGHT = '3a79fe865d7714ffb913e20123520a9fad6aad5244ae1b502146dabbc0b3371b'


def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()


def summarize(records):
    if not records:return {'queries': 0, 'metrics': None}
    return {'queries': len(records), 'metrics': {k: sum(r[k] for r in records)/len(records)
                                                for k in ['mrr', 'hit1', 'hit10', 'recall1', 'recall10']}}


def save_matrix(root, name, value):
    result = {};value = value.tocsr();value.sum_duplicates();value.sort_indices()
    for key, array in [('data', value.data), ('indices', value.indices), ('indptr', value.indptr)]:
        path = root/f'{name}-{key}.npy';np.save(path, array, allow_pickle=False);result[path.name] = digest(path)
    return {'shape': list(value.shape), 'nnz': value.nnz, 'files': result}


def paired(records):
    if not records:return None
    # Aggregate query sums by meeting, then resample meetings jointly for both policies.
    ids = sorted({r['meeting'] for r in records});matrix = []
    for identity in ids:
        subset = [r for r in records if r['meeting'] == identity]
        matrix.append([len(subset), sum(r['policies']['current']['mrr'] for r in subset),
                       sum(r['policies']['context']['mrr'] for r in subset)])
    rng = np.random.default_rng(20261002);a = np.array(matrix)
    totals = a[rng.integers(0, len(a), size=(1000, len(a)))].sum(axis=1)
    gain = (totals[:, 2]-totals[:, 1])/totals[:, 0]
    return {'draws': 1000, 'seed': 20261002, 'mrr_gain_percentiles': np.percentile(gain, [2.5, 50, 97.5]).tolist(),
            'limitation': 'descriptive train-meeting resampling; source families/pretraining/selection uncertainty not covered'}


def experiment(a):
    started = time.perf_counter();cpu = time.process_time()
    if digest(a.preflight) != PREFLIGHT:raise ValueError('frozen complete original QMSum preflight required')
    preflight = json.loads(a.preflight.read_text());capture_pin, source_pin, meetings = load(a.source)
    if capture_pin != preflight['capture_sha256'] or source_pin != preflight['source_sha256']:
        raise ValueError('original QMSum capture drift')
    for name, pin in preflight['code_pins'].items():
        if digest(Path(__file__).with_name(name)) != pin:raise ValueError('frozen source implementation drift')
    lengths = [len(m.turns) for m in meetings];texts = [t.text for m in meetings for t in m.turns]
    tokens = np.array([len(TOKEN.findall(t.lower())) for t in texts], dtype=np.int64)
    records = contexts(tokens, lengths);preflight_ms = (time.perf_counter()-started)*1000;begin = time.perf_counter()
    vectorizer = CountVectorizer(lowercase=True, token_pattern=TOKEN.pattern, ngram_range=(1, 1), max_features=20000)
    counts = vectorizer.fit_transform(texts);counts.sum_duplicates();counts.sort_indices()
    transformer = TfidfTransformer(norm='l2', use_idf=True, smooth_idf=True, sublinear_tf=True).fit(counts)
    current = transformer.transform(counts);combined = aggregate(counts, records);context = transformer.transform(combined)
    fit_ms = (time.perf_counter()-begin)*1000;begin = time.perf_counter()
    matrices = {name: save_matrix(a.output_directory, name, value) for name, value in
                [('counts', counts), ('current', current), ('context', context)]}
    vocabulary = vectorizer.get_feature_names_out().tolist()
    with (a.output_directory/'vocabulary.json').open('x') as f:json.dump(vocabulary, f, ensure_ascii=False)
    np.save(a.output_directory/'idf.npy', transformer.idf_, allow_pickle=False)
    write_ms = (time.perf_counter()-begin)*1000;begin = time.perf_counter();offset = 0;queries = [];support = Counter();purity = Counter()
    common = (current.getnnz(axis=1) > 0) & (context.getnnz(axis=1) > 0)
    for meeting in meetings:
        size = len(meeting.turns);members, _ = membership(meeting)
        labels = np.array([m[0] if len(m) == 1 else -1 for m in members], dtype=np.int64)
        original = np.arange(size);singleton = labels >= 0;nonempty = common[offset:offset+size]
        eligible = singleton & (tokens[offset:offset+size] >= 5) & nonempty
        support['original_turns'] += size;support['singleton_annotated_turns'] += int(singleton.sum())
        support['eligible_queries'] += int(eligible.sum());support['empty_source_vectors'] += int((~nonempty).sum())
        for i in np.flatnonzero(singleton):
            record = records[offset+i];source_members = members[record.start-offset:i+1]
            if all(m == (int(labels[i]),) for m in source_members):purity['same_singleton_only'] += 1
            else:purity['different_or_unknown_memberships'] += 1
        indices = np.flatnonzero(eligible)
        for start in range(0, len(indices), 128):
            block = indices[start:start+128]
            query = current[offset+block]
            scores = {name: (query @ value[offset:offset+size].T).toarray()
                      for name, value in [('current', current), ('context', context)]}
            for row, i in enumerate(block):
                mask = singleton & nonempty & (np.abs(original-i) > 4)
                candidates = original[mask];relevant = labels[mask] == labels[i]
                record = {'meeting': meeting.ordinal, 'ordinal': int(i), 'vector_row': offset+int(i),
                          'candidates': len(candidates), 'relevant': int(relevant.sum()),
                          'candidate_prevalence': float(relevant.mean()) if len(relevant) else None,
                          'informative': bool(relevant.any() and (~relevant).any()), 'policies': {}}
                if not len(candidates):record['unsupported'] = 'no_nonoverlapping_candidate'
                elif not relevant.any():record['unsupported'] = 'no_known_same_topic_peer'
                else:
                    record['unsupported'] = None
                    for name in ['current', 'context']:record['policies'][name] = rank(scores[name][row, mask], candidates, relevant)
                queries.append(record)
        offset += size
        if meeting.ordinal % 25 == 0:print(json.dumps({'processed_meetings': meeting.ordinal+1, 'turns': offset}), flush=True)
    if offset != len(texts):raise ValueError('incomplete original source coverage')
    scopes = {'supported': [r for r in queries if r['unsupported'] is None], 'informative': [r for r in queries if r['informative']]}
    summary = {scope: {'policies': {name: summarize([r['policies'][name] for r in rows]) for name in ['current', 'context']},
                       'chance_hit1': sum(r['candidate_prevalence'] for r in rows)/len(rows) if rows else None,
                       'paired': paired(rows)} for scope, rows in scopes.items()}
    result = {'schema_version': 1, 'scope': 'in_sample_source_representation_diagnostic_not_topic_selection',
              'python': platform.python_version(), 'sklearn': sklearn.__version__, 'source_sha256': source_pin,
              'capture_sha256': capture_pin, 'preflight_sha256': PREFLIGHT,
              'protocol': {'vocabulary_cap': 20000, 'ngram': [1, 1], 'token_pattern': TOKEN.pattern, 'stopwords': None,
                           'idf_fit': 'all_original_training_turn_counts', 'smooth_idf': True, 'sublinear_tf': True,
                           'norm': 'l2', 'maximum_context_turns': 5, 'context_token_cap': 1024,
                           'current_over_cap': 'retain_full_current_and_borrow_no_context', 'minimum_query_tokens': 5,
                           'nearby_exclusion_radius': 4, 'query_representation': 'current_for_both',
                           'tie': 'score_descending_then_original_ordinal', 'new_inference_calls': 0,
                           'held_out_sources_read': False, 'annotation_features': False},
              'matrices': matrices, 'vocabulary_sha256': digest(a.output_directory/'vocabulary.json'),
              'idf_sha256': digest(a.output_directory/'idf.npy'), 'vocabulary_size': len(vocabulary),
              'contexts': [asdict(r) for r in records], 'context_limits': dict(Counter(r.limit for r in records)),
              'support': dict(support), 'context_purity_on_singleton_anchors': dict(purity),
              'unsupported_queries': dict(Counter(r['unsupported'] for r in queries if r['unsupported'])),
              'summary': summary, 'queries': queries,
              'code_pins': {name: digest(Path(__file__).with_name(name)) for name in
                            ['qmsum_lexical_context.py', 'causal_lexical_context.py', 'qmsum.py', 'uv.lock']}}
    result['timings'] = {'preflight_ms': preflight_ms, 'fit_aggregate_ms': fit_ms, 'matrix_write_ms': write_ms,
                         'evaluation_ms': (time.perf_counter()-begin)*1000, 'total_ms': (time.perf_counter()-started)*1000,
                         'process_cpu_ms': (time.process_time()-cpu)*1000, 'peak_rss_kib': resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}
    return result


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for key in ['source', 'preflight', 'output-directory']:p.add_argument('--'+key, required=True, type=Path)
    p.add_argument('--compare', type=Path);a = p.parse_args()
    if a.output_directory.exists() or a.output_directory.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git output required')
    a.output_directory.mkdir(parents=True);r = experiment(a)
    if a.compare:compare(json.loads(a.compare.read_text()), r)
    with (a.output_directory/'results.json').open('x') as f:json.dump(r, f, indent=2, allow_nan=False)
    print(json.dumps({'summary': r['summary'], 'support': r['support'], 'purity': r['context_purity_on_singleton_anchors'],
                      'context_limits': r['context_limits'], 'timings': r['timings']}), flush=True)


if __name__ == '__main__':main()
