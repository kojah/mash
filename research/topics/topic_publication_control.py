"""Frozen physical-arrival replay; excludes native ingestion and inference."""
import argparse
from concurrent.futures import ProcessPoolExecutor
import hashlib
import json
import math
import multiprocessing
from pathlib import Path
import resource
import time

from memory_audit import sample
from topic_phrases import PhraseSource
from topic_phrase_store import PhraseStore
from topic_publication import Publisher, build_snapshot


PINS = {
    'checkpoint': '3ab25cafacab7ef1c6c0682189740aa23c620e4eab994db36c6d36f5e0d6d1f2',
    'capture': '8d06df371de287a4e12ab92376b276af111a63c7ffaeb58f447377afc51005aa',
    'baseline': 'e1e425b2d7f635df16e07f8f840e44f3da4f108899d4de233095325901a246c5',
}
CODE = ['PUBLICATION_PROTOCOL.md', 'topic_publication.py', 'topic_publication_control.py',
        'topic_phrase_store.py', 'topic_phrases.py', 'phrase_selection.py',
        'subject_labels.py', 'topic_features.py', 'label_noise.py',
        'topic_background.py', 'memory_audit.py']


def digest(path):
    with path.open('rb') as file:
        return hashlib.file_digest(file, 'sha256').hexdigest()


def semantics(snapshot):
    return {key: value for key, value in snapshot.items()
            if key not in {'revision', 'versions'}}


def load(args):
    hashes = {name: digest(getattr(args, name)) for name in PINS}
    if hashes != PINS:
        raise ValueError('frozen original inputs required')
    records = json.loads(args.checkpoint.read_text())['records']
    capture = json.loads(args.capture.read_text())
    provenance = {row['id']: row for row in capture['source_provenance']}
    if len(records) != 1850 or len(provenance) != 1850:
        raise ValueError('complete frozen source cohort required')
    sources = []
    for row in records:
        original = provenance[row['id']]
        if (row['thread'] != original['thread']
                or hashlib.sha256(row['text'].encode()).hexdigest()
                != original['represented_text_sha256']):
            raise ValueError('original source provenance differs')
        sources.append(PhraseSource(row['id'], row['thread'], row['provider'], row['text']))
    if len({row.id for row in sources}) != len(sources):
        raise ValueError('duplicate source identity')
    baseline = json.loads(args.baseline.read_text())['outputs']['cohort']['source_unit_candidate_coverage']
    return sources, baseline, capture['source_ledger_sha256']


def percentile95(values):
    return sorted(values)[math.ceil(.95 * len(values)) - 1]


def run(args):
    root = Path(__file__).parent
    pins = {name: digest(root / name) for name in CODE}
    started, cpu = time.monotonic(), time.process_time()
    child_cpu = resource.getrusage(resource.RUSAGE_CHILDREN)
    sources, baseline, ledger = load(args)
    loaded_ms = 1000 * (time.monotonic() - started)
    model = PhraseStore(baseline['namespace'])
    admitted, lags, drift, publications, memory = [], [], [], [], []
    workers = set()
    max_revision_backlog = max_jobs = max_pending = 0
    sampled_at = 0.
    published_revision = 0
    spawn = multiprocessing.get_context('spawn')
    with ProcessPoolExecutor(max_workers=1, mp_context=spawn) as pool:
        publisher = Publisher(model, lambda checkpoint: pool.submit(build_snapshot, checkpoint))
        stream_start = time.monotonic()

        def tick():
            nonlocal max_revision_backlog, max_jobs, max_pending, sampled_at, published_revision
            result = publisher.poll()
            now = time.monotonic()
            if now - stream_start > 180:
                raise TimeoutError('control deadline exceeded; do not restart a live worker')
            if result is not None:
                revision = result.snapshot['revision']
                lags.extend(1000 * (now - admitted[index])
                            for index in range(published_revision, revision))
                published_revision = revision
                workers.add(result.pid)
                publications.append({'revision': revision, 'admitted': len(admitted),
                    'elapsed_ms': 1000 * (now - stream_start),
                    'topics': len(result.snapshot['topics']),
                    'worker_wall_ms': result.wall_ms, 'worker_cpu_ms': result.cpu_ms,
                    'worker_rusage_peak_kib': result.peak_rss_kib})
            max_revision_backlog = max(max_revision_backlog, model.revision - max(0, publisher.published_revision))
            max_jobs, max_pending = max(max_jobs, publisher.jobs), max(max_pending, publisher.pending)
            if publisher.jobs > 1 or publisher.pending > 1:
                raise ValueError('publication work queue is unbounded')
            if now - sampled_at >= .05:
                import os
                for pid in [os.getpid(), *sorted(workers)]:
                    try:
                        memory.append({'pid': pid, 'elapsed_ms': 1000 * (now - stream_start), **sample(pid)})
                    except (FileNotFoundError, ProcessLookupError):
                        pass
                sampled_at = now

        for index, source in enumerate(sources):
            deadline = stream_start + index / 100
            while time.monotonic() < deadline:
                tick()
                time.sleep(min(.005, max(0, deadline - time.monotonic())))
            if not model.upsert(source):
                raise ValueError('captured source unexpectedly idempotent')
            admitted.append(time.monotonic())
            drift.append(1000 * (admitted[-1] - deadline))
            tick()
        arrivals_end = time.monotonic()
        while not publisher.drained:
            tick()
            if not publisher.drained:
                time.sleep(.005)
        drained_at = time.monotonic()
        final = publisher.published.snapshot
        checkpoint = model.checkpoint()
    if semantics(final) != semantics(baseline):
        raise ValueError('drained lexical baseline differs')
    if final['versions'] != model.versions or final['revision'] != len(sources):
        raise ValueError('drained source versions differ')
    if len(lags) != len(sources) or len(workers) != 1:
        raise ValueError('incomplete source lag or worker accounting')
    if pins != {name: digest(root / name) for name in CODE} or PINS != {name: digest(getattr(args, name)) for name in PINS}:
        raise ValueError('source or code changed during capture')
    import os
    parent_pid = os.getpid()
    scopes = {}
    for name, pids in [('parent', {parent_pid}), ('worker', workers)]:
        rows = [row for row in memory if row['pid'] in pids]
        if not rows:
            raise ValueError('missing external memory scope')
        scopes[name] = {'samples': len(rows),
            'maximum_kib': {key: max(row[key] for row in rows)
                            for key in ['VmRSS', 'VmHWM', 'Rss', 'Pss']}}
    child_end = resource.getrusage(resource.RUSAGE_CHILDREN)
    return {'schema_version': 1, 'scope': 'frozen_phrase_publication_not_complete_ingestion',
        'input_hashes': PINS, 'code_pins': pins, 'source_ledger_sha256': ledger,
        'arrival_rate_per_second': 100, 'poll_interval_ms': 5,
        'final_snapshot': final, 'final_checkpoint': checkpoint,
        'checks': {'baseline_semantics_exact': True, 'source_versions_exact': True,
            'all_updates_accounted': True, 'final_drained': True,
            'published_before_final_admission': any(row['admitted'] < len(sources)
                and row['topics'] > 0 for row in publications)},
        'semantic_quality_verified': False, 'full_ingestion_cost_verified': False,
        'production_selected': False,
        'runtime': {'load_ms': loaded_ms, 'total_ms': 1000 * (time.monotonic() - started),
            'parent_cpu_ms': 1000 * (time.process_time() - cpu),
            'exited_child_cpu_ms': 1000 * (child_end.ru_utime + child_end.ru_stime
                - child_cpu.ru_utime - child_cpu.ru_stime),
            'admission_ms': 1000 * (arrivals_end - stream_start),
            'drain_ms': 1000 * (drained_at - arrivals_end),
            'lag_p95_ms_nearest_rank': percentile95(lags), 'lag_max_ms': max(lags),
            'admission_drift_p95_ms': percentile95(drift), 'admission_drift_max_ms': max(drift),
            'max_revision_backlog': max_revision_backlog, 'max_jobs': max_jobs,
            'max_pending': max_pending, 'publications': publications, 'memory': scopes,
            'memory_samples': memory,
            'parent_rusage_peak_kib': resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['checkpoint', 'capture', 'baseline', 'output']:
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--compare', type=Path)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh private output outside Git required')
    result = run(args)
    if args.compare:
        previous = json.loads(args.compare.read_text())
        if ({key: value for key, value in previous.items() if key != 'runtime'}
                != {key: value for key, value in result.items() if key != 'runtime'}):
            raise ValueError('non-runtime final-state repeat differs')
    with args.output.open('x') as output:
        json.dump(result, output, sort_keys=True, allow_nan=False)
    args.output.chmod(0o600)
    print(json.dumps({'complete': True, 'checks': result['checks'],
        'publications': len(result['runtime']['publications']),
        'admission_ms': result['runtime']['admission_ms'],
        'drain_ms': result['runtime']['drain_ms'],
        'lag_p95_ms': result['runtime']['lag_p95_ms_nearest_rank']}))
