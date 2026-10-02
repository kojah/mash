#!/usr/bin/env python3
"""Fixed Linux whole-index ancestry-heavy control; sampled peaks are lower bounds."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import sqlite3
import subprocess
import time


CONTENT = 'Inspect metadata retention during indexing.'
CASES = [(0, 1), (100_000, 1), (300_000, 1), (100_000, 2)]


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def fixture(root, events, sources):
    roots = root / 'claude'
    roots.mkdir()
    hashes = {}
    for source in range(sources):
        path = roots / f'probe-{source}.jsonl'
        session = f'memory-probe-{source}'
        parent = None
        with path.open('w') as stream:
            for index in range(events):
                key = f'event-{index:09d}'
                stream.write(json.dumps({'type': 'progress', 'sessionId': session,
                                         'uuid': key, 'parentUuid': parent},
                                        separators=(',', ':')) + '\n')
                parent = key
            stream.write(json.dumps({'type': 'user', 'sessionId': session,
                                     'uuid': 'dialogue', 'parentUuid': parent,
                                     'message': {'role': 'user', 'content': CONTENT}},
                                    separators=(',', ':')) + '\n')
        hashes[path.name] = {'sha256': digest(path), 'bytes': path.stat().st_size}
    config = root / 'config.toml'
    config.write_text('version = 1\n[index]\nall_history = true\n'
                      '[providers.claude-code]\nroots = ' + json.dumps([str(roots)]) + '\n')
    return config, hashes


def counters(path, names):
    result = {}
    for line in path.read_text().splitlines():
        key, _, value = line.partition(':')
        if key in names:
            fields = value.split()
            if len(fields) != 2 or fields[1] != 'kB' or not fields[0].isdigit():
                raise ValueError('invalid proc memory units')
            result[key] = int(fields[0])
    if set(result) != set(names):
        raise ProcessLookupError('process address space ended')
    return result


def measure(command, binary, root):
    samples, gaps = [], 0
    started = time.monotonic()
    with (root / 'stdout.json').open('wb') as stdout, (root / 'stderr.jsonl').open('wb') as stderr:
        child = subprocess.Popen(command, stdout=stdout, stderr=stderr,
                                 env={**os.environ, 'MASH_TRACE': '1'})
        try:
            while True:
                pid, status, usage = os.wait4(child.pid, os.WNOHANG)
                if pid:
                    child.returncode = os.waitstatus_to_exitcode(status)
                    break
                proc = Path('/proc') / str(child.pid)
                try:
                    if (proc / 'exe').resolve(strict=True) == binary:
                        sample = counters(proc / 'status', {'VmRSS', 'VmHWM'})
                        sample.update(counters(proc / 'smaps_rollup', {'Rss', 'Pss'}))
                        samples.append({'elapsed_ms': (time.monotonic() - started) * 1000,
                                        **sample})
                except (FileNotFoundError, ProcessLookupError):
                    gaps += 1
                time.sleep(.02)
        finally:
            if child.returncode is None:
                child.kill()
                _, status, _ = os.wait4(child.pid, 0)
                child.returncode = os.waitstatus_to_exitcode(status)
    if child.returncode != 0 or not samples:
        raise RuntimeError('index failed or no current-image memory samples; inspect retained logs')
    return {'exit_code': child.returncode, 'elapsed_seconds': time.monotonic() - started,
            'cpu_user_seconds': usage.ru_utime, 'cpu_system_seconds': usage.ru_stime,
            'wait4_maxrss_kib': usage.ru_maxrss, 'sample_count': len(samples),
            'vanished_reads': gaps, 'interval_ms': 20,
            'sampled_maximum_kib': {key: max(sample[key] for sample in samples)
                                    for key in ('VmRSS', 'VmHWM', 'Rss', 'Pss')},
            'samples': samples}


def run(args):
    if platform.system() != 'Linux':
        raise ValueError('requires Linux procfs and wait4')
    binary, models, output = args.binary.resolve(strict=True), args.models.resolve(strict=True), args.output.resolve()
    output.mkdir()
    binary_hash = digest(binary)
    results = []
    for events, sources in CASES:
        root = output / f'events-{events}-sources-{sources}'
        root.mkdir()
        config, hashes = fixture(root, events, sources)
        database = root / 'index.sqlite3'
        measurement = measure([str(binary), '--config', str(config), '--db', str(database),
                               '--models-dir', str(models), 'index'], binary, root)
        indexed = json.loads((root / 'stdout.json').read_text())
        with sqlite3.connect(database.as_uri() + '?mode=ro', uri=True) as connection:
            rows = connection.execute('SELECT id,role,content FROM messages ORDER BY id').fetchall()
            vectors = connection.execute('SELECT COUNT(*) FROM message_embeddings').fetchone()[0]
            conversations = connection.execute('SELECT COUNT(*) FROM conversations').fetchone()[0]
        if (len(rows), vectors, conversations) != (sources, sources, sources):
            raise ValueError('canonical/vector counts do not match the complete fixture')
        if any(role != 'user' or content != CONTENT for _, role, content in rows):
            raise ValueError('canonical message changed')
        progress = [json.loads(line) for line in (root / 'stderr.jsonl').read_text().splitlines()]
        admission = [event['parser_admission'] for event in progress if 'parser_admission' in event]
        result = {'events_per_source': events, 'sources': sources, 'input_files': hashes,
                  'canonical_sha256': hashlib.sha256(json.dumps(rows).encode()).hexdigest(),
                  'canonical_messages': len(rows), 'stored_vectors': vectors,
                  'index_result': indexed, 'parser_admission': admission[-1] if admission else None,
                  'measurement': measurement}
        (root / 'measurement.json').write_text(json.dumps(result, indent=2) + '\n')
        results.append(result)
        print(json.dumps({'events': events, 'sources': sources,
                          'rss_kib': measurement['sampled_maximum_kib']['Rss'],
                          'elapsed_seconds': measurement['elapsed_seconds']}), flush=True)
    if digest(binary) != binary_hash:
        raise ValueError('binary changed during the fixed matrix')
    (output / 'capture.json').write_text(json.dumps({
        'scope': 'linux-whole-index-ancestry-control-v1', 'python': platform.python_version(),
        'platform': platform.platform(), 'binary_sha256': binary_hash,
        'script_sha256': digest(Path(__file__).resolve()), 'tracing': True, 'results': results,
        'limitations': [
            'fixed synthetic ancestry-heavy control, not representative corpus or fleet performance',
            'full local indexing with installed models, source processing, inference and persistence',
            'current-image RSS/PSS sampled maxima are lower bounds and exclude child processes/observer',
            'status RSS/HWM counters are approximate; smaps snapshots describe observation times',
            'wait4 highwater includes pre-exec lifetime; no peak subtraction or stage attribution',
            'tracing and procfs sampling can perturb timing; no ingestion throughput acceptance claim',
            'operational ancestry varies; dialogue keys, replay duplicates and other providers remain untested',
        ],
    }, indent=2) + '\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--models', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    run(parser.parse_args())


if __name__ == '__main__':
    main()
