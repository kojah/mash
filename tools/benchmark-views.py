#!/usr/bin/env python3
"""Private, bounded real-session local silver/views cost comparison on Linux.

Uses explicitly installed models and stable copies of provider files. Never
changes the live index or calls remote inference. Captures contain private data;
keep the output outside Git. Process-cold CLI timings are not filesystem-cold
or resident-worker timings. The source sample is biased toward supported files
under 8 MiB, stratified by provider and source size, not a fleet-wide workload.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import shutil
import sqlite3
import statistics
import subprocess
import threading
import time


BINS = ((1024, 64 * 1024), (64 * 1024, 512 * 1024),
        (512 * 1024, 2 * 1024**2), (2 * 1024**2, 8 * 1024**2))
SILVER = ('conversations', 'messages', 'conversation_enrichment',
          'source_checkpoints', 'claude_sources', 'claude_source_messages',
          'silver_state', 'silver_scans', 'silver_rejections', 'tombstones')
QUERIES = ('cbioportal', 'rust', 'qdrant', 'search', 'embedding', 'configuration')


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, allow_nan=False) + '\n')


def snapshot(output, count):
    roots = {'codex': Path.home() / '.codex/sessions',
             'claude-code': Path.home() / '.claude/projects'}
    selected = []
    for provider, source in roots.items():
        files = sorted(source.rglob('*.jsonl'), key=lambda p: hashlib.sha256(
            ('mash-views-cost-v1:' + str(p.relative_to(source))).encode()).digest())
        for low, high in BINS:
            kept = 0
            for path in files:
                before = path.stat()
                if not low <= before.st_size < high:
                    continue
                destination = output / 'corpus' / provider / path.relative_to(source)
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(path, destination)
                after = path.stat()
                if (before.st_size, before.st_mtime_ns, before.st_ino) != (
                        after.st_size, after.st_mtime_ns, after.st_ino):
                    destination.unlink()
                    continue
                selected.append({'provider': provider, 'size_bin': [low, high],
                                 'bytes': before.st_size, 'sha256': sha(destination),
                                 'relative_path': str(path.relative_to(source))})
                kept += 1
                if kept == count:
                    break
    if not selected:
        raise ValueError('no stable source files in the bounded strata')
    write_json(output / 'private-source-manifest.json', selected)
    return selected


def config(output, representation):
    path = output / f'{representation}.toml'
    path.write_text('version=1\n[index]\nall_history=true\n' + ''.join(
        f'[providers.{provider}]\nroots={json.dumps([str(output / "corpus" / provider)])}\n'
        for provider in ('codex', 'claude-code')) +
        f'[views.build]\nrepresentations="{representation}"\n' +
        '[views.query]\nintent="exact-mention"\n')
    return path


def connection(database):
    return sqlite3.connect(database.as_uri() + '?mode=ro', uri=True)


def silver_digest(database):
    digest = hashlib.sha256()
    with connection(database) as db:
        for table in SILVER:
            columns = [row[1] for row in db.execute(f'PRAGMA table_info({table})')]
            if not columns:
                raise ValueError('missing canonical table ' + table)
            digest.update(json.dumps([table, columns]).encode())
            order = ','.join('"' + name + '"' for name in columns)
            for row in db.execute(f'SELECT * FROM {table} ORDER BY {order}'):
                digest.update(json.dumps(row, ensure_ascii=False, allow_nan=False).encode())
                digest.update(b'\n')
    return digest.hexdigest()


def storage(database):
    with connection(database) as db:
        page = db.execute('PRAGMA page_size').fetchone()[0]
        pages = db.execute('PRAGMA page_count').fetchone()[0]
        free = db.execute('PRAGMA freelist_count').fetchone()[0]
        tables = {name: {'allocated_bytes': allocated, 'payload_bytes': payload}
                  for name, allocated, payload in db.execute(
                      'SELECT name,sum(pgsize),sum(payload) FROM dbstat GROUP BY name')}
        counts = {table: db.execute(f'SELECT count(*) FROM {table}').fetchone()[0]
                  for table in ('conversations', 'messages', 'message_embeddings')}
    wal = database.with_name(database.name + '-wal')
    return {'database_file_bytes': database.stat().st_size,
            'wal_file_bytes': wal.stat().st_size if wal.exists() else 0,
            'logical_database_bytes': page * pages, 'free_page_bytes': page * free,
            'tables': tables, 'counts': counts}


def measure(command, binary, directory):
    directory.mkdir()
    samples = []
    stop = threading.Event()
    with (directory / 'stdout.json').open('wb') as stdout, (
            directory / 'stderr.jsonl').open('wb') as stderr:
        started = time.perf_counter()
        process = subprocess.Popen(command, stdout=stdout, stderr=stderr)

        def monitor():
            proc = Path('/proc') / str(process.pid)
            while not stop.is_set():
                try:
                    if (proc / 'exe').resolve(strict=True) == binary:
                        values = {}
                        for filename in ('status', 'smaps_rollup'):
                            for line in (proc / filename).read_text().splitlines():
                                key, _, value = line.partition(':')
                                if key in ('VmRSS', 'VmHWM', 'Rss', 'Pss'):
                                    values[key] = int(value.split()[0])
                        samples.append(values)
                except (OSError, ValueError):
                    pass
                stop.wait(.02)

        thread = threading.Thread(target=monitor, daemon=True)
        thread.start()
        try:
            _, status, usage = os.wait4(process.pid, 0)
            elapsed = time.perf_counter() - started
            process.returncode = os.waitstatus_to_exitcode(status)
        finally:
            stop.set()
            thread.join()
        if process.returncode:
            raise RuntimeError(f'command failed; inspect private logs in {directory}')
    result = json.loads((directory / 'stdout.json').read_text())
    measurement = {'wall_seconds': elapsed, 'cpu_user_seconds': usage.ru_utime,
                   'cpu_system_seconds': usage.ru_stime,
                   'wait4_maxrss_kib': usage.ru_maxrss, 'sample_interval_ms': 20,
                   'sample_count': len(samples),
                   'current_image_sampled_maximum_kib': {
                       key: max((s[key] for s in samples if key in s), default=None)
                       for key in ('VmRSS', 'VmHWM', 'Rss', 'Pss')}}
    write_json(directory / 'measurement.json', measurement)
    return result, measurement


def run(args):
    if platform.system() != 'Linux' or not 1 <= args.per_bin <= 16 or args.repetitions < 1:
        raise ValueError('requires Linux, per-bin 1..16 and positive repetitions')
    binary = args.binary.resolve(strict=True)
    models = args.models.resolve(strict=True)
    output = args.output.resolve()
    if output.is_relative_to(Path(__file__).resolve().parent.parent):
        raise ValueError('private output must be outside the repository')
    output.mkdir(mode=0o700)
    selected = snapshot(output, args.per_bin)
    configs = {r: config(output, r) for r in ('lexical-only', 'lexical-and-dense')}
    database = output / 'index.sqlite3'
    binary_hash = sha(binary)
    base = [str(binary), '--db', str(database), '--models-dir', str(models)]
    initial, timing = measure(base + ['--config', str(configs['lexical-only']), 'index'],
                              binary, output / 'initial-silver-and-lexical')
    if initial['silver']['completeness'] != 'complete' or initial['malformed_records']:
        raise ValueError('sample did not produce certified complete silver')
    canonical = silver_digest(database)
    stages = [{'name': 'initial-silver-and-lexical', 'index': initial,
               'measurement': timing, 'storage': storage(database)}]
    build_seconds = [timing['wall_seconds']]
    shutil.rmtree(output / 'corpus')  # Only this run's owned copies; provider sources stay intact.
    for number, representation in enumerate(('lexical-and-dense', 'lexical-only', 'lexical-and-dense')):
        result, timing = measure(base + ['--config', str(configs[representation]),
                                        'index', '--rebuild-local-views'],
                                 binary, output / f'rebuild-{number}-{representation}')
        if silver_digest(database) != canonical or result['scanned_files']:
            raise ValueError('views rebuild changed silver or scanned provider files')
        if representation == 'lexical-only' and result['model_inferences']:
            raise ValueError('lexical-only unexpectedly ran inference')
        stages.append({'name': representation, 'index': result,
                       'measurement': timing, 'storage': storage(database)})
        build_seconds.append(timing['wall_seconds'])
    searches = []
    for intent, ranking in (('exact-mention', None), ('related-discussion', 'fusion'),
                            ('related-discussion', 'contextual-rerank')):
        for provider in (None, 'codex'):
            for query in QUERIES:
                for repetition in range(args.repetitions):
                    command = base + ['--config', str(configs['lexical-and-dense']),
                                      'search', query, '--local', '--intent', intent]
                    if ranking:
                        command += ['--ranking', ranking]
                    if provider:
                        command += ['--provider', provider]
                    result, timing = measure(command, binary, output / f'search-{len(searches):03}')
                    searches.append({'intent': intent, 'ranking': ranking, 'provider': provider,
                                     'query': query, 'repetition': repetition, 'measurement': timing,
                                     'realized_mode': result['realized_mode']})
    if silver_digest(database) != canonical or sha(binary) != binary_hash:
        raise ValueError('search changed silver or benchmark binary changed')
    groups = []
    for intent, ranking in (('exact-mention', None), ('related-discussion', 'fusion'),
                            ('related-discussion', 'contextual-rerank')):
        for provider in (None, 'codex'):
            times = sorted(s['measurement']['wall_seconds'] for s in searches
                           if (s['intent'], s['ranking'], s['provider']) == (intent, ranking, provider))
            groups.append({'intent': intent, 'ranking': ranking, 'provider': provider,
                           'n': len(times), 'median_ms': statistics.median(times) * 1000,
                           'p95_ms': times[math.ceil(.95 * len(times))-1] * 1000})
    capture = {'scope': 'private-real-session-local-views-cost-v1',
               'platform': platform.platform(), 'python': platform.python_version(),
               'binary_sha256': binary_hash, 'script_sha256': sha(Path(__file__).resolve()),
               'installed_marker_sha256': sha(models / 'installed.json'),
               'source_files': len(selected), 'source_bytes': sum(s['bytes'] for s in selected),
               'silver_sha256': canonical, 'stages': stages, 'search_groups': groups,
               'searches': searches,
               'limitations': ['one Linux node, two providers, bounded size strata; not full fleet',
                               'CLI processes start fresh; filesystem/model pages may be cached',
                               '20ms current-image RSS/PSS sampled maxima are lower bounds',
                               'wait4 maxrss may include pre-exec peaks; do not subtract peaks',
                               'monitor overhead is included in wall time; child CPU excludes monitor',
                               'no worker, HTTP/shared backend, edits/deletes or quality evaluation']}
    write_json(output / 'capture.json', capture)
    print(json.dumps({'source_files': len(selected), 'counts': stages[0]['storage']['counts'],
                      'build_seconds': build_seconds,
                      'search_groups': groups}), flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--models', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--per-bin', type=int, default=4)
    parser.add_argument('--repetitions', type=int, default=3)
    run(parser.parse_args())
