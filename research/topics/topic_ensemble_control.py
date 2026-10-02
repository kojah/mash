"""Two frozen exposed-cohort arms; no semantic quality acceptance claim."""
import argparse
import hashlib
import json
from pathlib import Path
import resource
import time
import numpy as np
from topic_stream import Source
from topic_ensemble import Corroborator
from topic_phrases import PhraseSource, discover
from phrase_selection import Selection


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(args):
    root = Path(__file__).parent
    names = ['topic_ensemble.py', 'topic_ensemble_control.py', 'ENSEMBLE_PROTOCOL.md',
             'topic_phrases.py', 'phrase_selection.py', 'topic_features.py',
             'subject_labels.py', 'label_noise.py', 'topic_background.py', 'topic_stream.py']
    pins = {name: digest(root / name) for name in names}
    hashes = {name: digest(getattr(args, name)) for name in ['checkpoint', 'capture', 'baseline']}
    if hashes['checkpoint'] != '3ab25cafacab7ef1c6c0682189740aa23c620e4eab994db36c6d36f5e0d6d1f2' or hashes['capture'] != '8d06df371de287a4e12ab92376b276af111a63c7ffaeb58f447377afc51005aa':
        raise ValueError('frozen original capture required')
    started = time.perf_counter(); cpu = time.process_time()
    records = json.loads(args.checkpoint.read_text())['records']
    capture = json.loads(args.capture.read_text())
    provenance = {row['id']: row for row in capture['source_provenance']}
    if len(records) != 1850 or len(provenance) != 1850:
        raise ValueError('complete captured cohort required')
    sources = []
    for row in records:
        original = provenance[row['id']]
        if row['thread'] != original['thread'] or hashlib.sha256(row['text'].encode()).hexdigest() != original['represented_text_sha256']:
            raise ValueError('source provenance mismatch')
        sources.append(Source(row['id'], row['thread'], row['provider'], row['text'], np.array(row['vector'], dtype=np.float64)))
    old = json.loads(args.baseline.read_text())['outputs']['cohort']['source_unit_candidate_coverage']
    # The frozen store carries arrival counters; discover emits only semantics.
    old = {key: value for key, value in old.items() if key not in {'revision', 'versions'}}
    begin = time.perf_counter()
    snapshot = discover([PhraseSource(row.id, row.thread, row.provider, row.text) for row in sources], old['namespace'], selection=Selection.SOURCE_COVERAGE)
    if snapshot != old:
        raise ValueError('unchanged lexical baseline drift')
    lexical_ms = 1000 * (time.perf_counter() - begin)
    model = Corroborator(); latencies = []; prefixes = []
    for index, source in enumerate(sources):
        begin = time.perf_counter(); model.upsert(source)
        latencies.append(1000 * (time.perf_counter() - begin))
        if index + 1 in [1, 16, 128]:
            prefixes.append({'sources': index + 1, 'phrase_thread_buckets': len(model.buckets)})
    begin = time.perf_counter(); hybrid, decisions = model.apply(snapshot)
    apply_ms = 1000 * (time.perf_counter() - begin)
    # JSON checkpoint recreation and reverse arrivals must preserve every decision.
    checkpoint = json.loads(json.dumps(model.checkpoint()))
    restored = Corroborator.restore(checkpoint)
    if restored.apply(snapshot) != (hybrid, decisions):
        raise ValueError('checkpoint recreation differs')
    reverse = Corroborator.restore(list(reversed(checkpoint)))
    if reverse.apply(snapshot) != (hybrid, decisions):
        raise ValueError('reverse final geometry differs')
    first = sources[0]
    model.upsert(Source(first.id + '#duplicate', first.thread, first.provider, first.text, first.vector))
    if model.apply(snapshot) != (hybrid, decisions):
        raise ValueError('duplicate vote differs')
    model.delete(first.id + '#duplicate')
    if model.checkpoint() != checkpoint:
        raise ValueError('duplicate cleanup differs')
    if pins != {name: digest(root / name) for name in names} or hashes != {name: digest(getattr(args, name)) for name in hashes}:
        raise ValueError('source/code drift')
    return {'schema_version': 1, 'scope': 'exposed_capture_phrase_dense_control_not_selection',
        'code_pins': pins, 'input_hashes': hashes, 'source_ledger_sha256': capture['source_ledger_sha256'],
        'arms': {'phrase_baseline': snapshot, 'phrase_dense': hybrid}, 'decisions': decisions,
        'incremental_geometry_prefixes': prefixes,
        'checks': {'baseline_exact': True, 'json_checkpoint_exact': True, 'reverse_geometry_exact': True,
                   'duplicate_vote_invariant': True, 'duplicate_cleanup_exact': True},
        'semantic_quality_verified': False, 'production_selected': False,
        'online_complete_pipeline_verified': False, 'full_ingestion_cost_verified': False,
        'runtime': {'lexical_snapshot_ms': lexical_ms, 'geometry_insert_total_ms': sum(latencies),
            'geometry_insert_p95_ms': float(np.percentile(latencies, 95)), 'apply_ms': apply_ms,
            'total_ms': 1000 * (time.perf_counter() - started), 'cpu_ms': 1000 * (time.process_time() - cpu),
            'rusage_peak_rss_kib': resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['checkpoint', 'capture', 'baseline', 'output']:
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--compare', type=Path)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh private outside-Git output required')
    result = run(args)
    if args.compare:
        prior = json.loads(args.compare.read_text()); current = dict(result)
        prior.pop('runtime'); current.pop('runtime')
        if prior != current:
            raise ValueError('non-runtime repeat differs')
    with args.output.open('x') as file:
        json.dump(result, file, sort_keys=True, allow_nan=False)
    args.output.chmod(0o600)
    print(json.dumps({'complete': True, 'arms': {name: arm['diagnostics'] for name, arm in result['arms'].items()},
                      'runtime': result['runtime']}))
