"""Frozen longer captured-source lifecycle control, not production ingestion."""
import argparse
from concurrent.futures import ProcessPoolExecutor
from dataclasses import dataclass
import json
import multiprocessing
import os
from pathlib import Path
import resource
import time

from memory_audit import sample
from topic_phrase_store import PhraseStore
from topic_publication import Publisher, build_snapshot
from topic_publication_control import CODE, PINS, digest, load, percentile95, semantics


@dataclass(frozen=True)
class Mutation:
    action: str
    identity: str
    revision: int
    admitted: float
    window: int


def churn(sources, count=6000):
    if not sources or type(count) is not int or count < 0 or count % 2:
        raise ValueError('nonempty cohort and complete delete/reinsert pairs required')
    for index in range(count):
        yield ('delete' if index % 2 == 0 else 'upsert', sources[(index // 2) % len(sources)])


def represented(event, latest, versions):
    """Exact latest state versus a superseded transient event, not semantic utility."""
    if latest.get(event.identity) != (event.action, event.revision):
        return False
    if event.action == 'upsert':
        if versions.get(event.identity) != event.revision:
            raise ValueError('latest upsert version absent from captured publication')
    elif event.action == 'delete':
        if event.identity in versions:
            raise ValueError('latest deletion still present in captured publication')
    else:
        raise ValueError('unknown mutation action')
    return True


def verify(snapshot, revision, namespace, versions):
    if (snapshot['revision'] != revision or snapshot['namespace'] != namespace
            or snapshot['versions'] != versions
            or not set(snapshot['memberships']) <= versions.keys()):
        raise ValueError('captured publication source versions or scope differ')


def run(args):
    root = Path(__file__).parent
    code = [*CODE, 'PUBLICATION_CHURN_PROTOCOL.md', 'topic_publication_churn.py']
    pins = {name: digest(root / name) for name in code}
    started, cpu = time.monotonic(), time.process_time()
    child_start = resource.getrusage(resource.RUSAGE_CHILDREN)
    sources, baseline, ledger = load(args)
    loaded_ms = 1000 * (time.monotonic() - started)
    model = PhraseStore(baseline['namespace'])
    events, latest, expected, publications, memory = [], {}, {}, [], []
    windows = {index: {'events': 0, 'lags': [], 'drift': [], 'represented': 0,
        'superseded': 0, 'max_backlog': 0, 'end_backlog': 0} for index in range(-1, 6)}
    workers = set()
    sampled_at = 0.
    advanced = 0
    max_jobs = max_pending = max_expected = 0
    draining = False
    spawn = multiprocessing.get_context('spawn')
    with ProcessPoolExecutor(max_workers=1, mp_context=spawn) as pool:
        def submit(checkpoint):
            nonlocal max_expected
            revision = checkpoint['revision']
            versions = {row['source']['id']: row['version'] for row in checkpoint['records']}
            expected[revision] = (versions, dict(latest))
            max_expected = max(max_expected, len(expected))
            if len(expected) > 2:
                raise ValueError('diagnostic checkpoint metadata accumulated')
            return pool.submit(build_snapshot, checkpoint)
        publisher = Publisher(model, submit)
        stream_start = time.monotonic()

        def tick():
            nonlocal sampled_at, advanced, max_jobs, max_pending
            result = publisher.poll()
            now = time.monotonic()
            if now - stream_start > 240:
                raise TimeoutError('frozen control deadline exceeded; do not restart a live worker')
            if result is not None:
                revision = result.snapshot['revision']
                versions, captured_latest = expected.pop(revision)
                verify(result.snapshot, revision, model.namespace, versions)
                for event in events[advanced:revision]:
                    bucket = windows[event.window]
                    bucket['lags'].append(1000 * (now - event.admitted))
                    key = 'represented' if represented(event, captured_latest, versions) else 'superseded'
                    bucket[key] += 1
                advanced = revision
                workers.add(result.pid)
                publications.append({'revision': revision, 'admitted': len(events),
                    'elapsed_ms': 1000 * (now - stream_start),
                    'sources': len(versions), 'topics': len(result.snapshot['topics']),
                    'worker_wall_ms': result.wall_ms, 'worker_cpu_ms': result.cpu_ms,
                    'worker_rusage_peak_kib': result.peak_rss_kib})
            max_jobs, max_pending = max(max_jobs, publisher.jobs), max(max_pending, publisher.pending)
            if publisher.jobs > 1 or publisher.pending > 1:
                raise ValueError('unbounded publication jobs')
            if events and not draining:
                bucket = windows[events[-1].window]
                backlog = model.revision - max(0, publisher.published_revision)
                bucket['max_backlog'] = max(bucket['max_backlog'], backlog)
                bucket['end_backlog'] = backlog
            if now - sampled_at >= .05:
                for pid in [os.getpid(), *sorted(workers)]:
                    try:
                        memory.append({'pid': pid, 'elapsed_ms': 1000 * (now - stream_start), **sample(pid)})
                    except (FileNotFoundError, ProcessLookupError):
                        pass
                sampled_at = now

        def admit(action, source, window):
            deadline = stream_start + len(events) / 100
            while time.monotonic() < deadline:
                tick()
                time.sleep(min(.005, max(0, deadline - time.monotonic())))
            changed = model.delete(source.id) if action == 'delete' else model.upsert(source)
            if not changed:
                raise ValueError('scheduled mutation unexpectedly idempotent')
            now = time.monotonic()
            event = Mutation(action, source.id, model.revision, now, window)
            events.append(event)
            latest[source.id] = (action, model.revision)
            windows[window]['events'] += 1
            windows[window]['drift'].append(1000 * (now - deadline))
            tick()

        for source in sources:
            admit('upsert', source, -1)
        cold_end = time.monotonic()
        for index, (action, source) in enumerate(churn(sources)):
            admit(action, source, index // 1000)
        arrivals_end = time.monotonic()
        draining = True
        while not publisher.drained:
            tick()
            if not publisher.drained:
                time.sleep(.005)
        drained_at = time.monotonic()
        final, checkpoint = publisher.published.snapshot, model.checkpoint()
    if (model.revision != 7850 or len(events) != 7850 or advanced != 7850
            or len(workers) != 1 or expected or semantics(final) != semantics(baseline)
            or final['versions'] != model.versions):
        raise ValueError('incomplete drained baseline or lifecycle accounting')
    if pins != {name: digest(root / name) for name in code} or PINS != {name: digest(getattr(args, name)) for name in PINS}:
        raise ValueError('source or code changed during capture')
    scopes = {}
    for name, pids in [('parent', {os.getpid()}), ('worker', workers)]:
        rows = [row for row in memory if row['pid'] in pids]
        if not rows:
            raise ValueError('missing sampled memory scope')
        scopes[name] = {'samples': len(rows), 'maximum_kib': {
            key: max(row[key] for row in rows) for key in ['VmRSS', 'VmHWM', 'Rss', 'Pss']}}
    summaries = []
    for index, bucket in windows.items():
        if len(bucket['lags']) != bucket['events']:
            raise ValueError('missing event advancement accounting')
        summaries.append({'window': index, **{key: value for key, value in bucket.items() if key not in {'lags', 'drift'}},
            'advancement_lag_p95_ms': percentile95(bucket['lags']),
            'advancement_lag_max_ms': max(bucket['lags']),
            'admission_drift_p95_ms': percentile95(bucket['drift']), 'admission_drift_max_ms': max(bucket['drift'])})
    child_end = resource.getrusage(resource.RUSAGE_CHILDREN)
    return {'schema_version': 1, 'scope': 'captured_churn_publication_not_complete_ingestion',
        'input_hashes': PINS, 'code_pins': pins, 'source_ledger_sha256': ledger,
        'schedule': {'initial': 1850, 'churn': 6000, 'events_per_second': 100, 'poll_ms': 5},
        'final_snapshot': final, 'final_checkpoint': checkpoint,
        'checks': {'baseline_semantics_exact': True, 'source_versions_exact': True,
            'every_publication_versions_exact': True, 'all_events_accounted': True, 'final_drained': True},
        'semantic_quality_verified': False, 'full_ingestion_cost_verified': False, 'production_selected': False,
        'runtime': {'load_ms': loaded_ms, 'total_ms': 1000 * (time.monotonic() - started),
            'cold_admission_ms': 1000 * (cold_end - stream_start),
            'churn_admission_ms': 1000 * (arrivals_end - cold_end), 'drain_ms': 1000 * (drained_at - arrivals_end),
            'parent_cpu_ms': 1000 * (time.process_time() - cpu),
            'exited_child_cpu_ms': 1000 * (child_end.ru_utime + child_end.ru_stime - child_start.ru_utime - child_start.ru_stime),
            'max_jobs': max_jobs, 'max_pending': max_pending, 'max_diagnostic_expectations': max_expected,
            'windows': summaries, 'publications': publications, 'memory': scopes, 'memory_samples': memory,
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
        'total_ms': result['runtime']['total_ms'], 'drain_ms': result['runtime']['drain_ms'],
        'publications': len(result['runtime']['publications'])}), flush=True)
