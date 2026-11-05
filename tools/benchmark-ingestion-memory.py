#!/usr/bin/env python3
"""Fixed Linux whole-index ancestry-heavy control; sampled peaks are lower bounds."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import subprocess
import time
from typing import TypedDict

SPEC = importlib.util.spec_from_file_location('views_cost', Path(__file__).with_name('benchmark-views.py'))
if SPEC is None or SPEC.loader is None:
    raise ImportError('benchmark-views.py could not be loaded')
COST = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(COST)


CONTENT = 'Inspect metadata retention during indexing.'
CASES = [(0, 1), (100_000, 1), (300_000, 1), (100_000, 2)]


class SourceCapture(TypedDict):
    sha256: str
    bytes: int


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def fixture(root: Path, events: int, sources: int, qdrant: str) -> tuple[Path, dict[str, SourceCapture]]:
    roots = root / 'claude'
    roots.mkdir()
    hashes: dict[str, SourceCapture] = {}
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
                      '[providers.claude-code]\nroots = ' + json.dumps([str(roots)]) + '\n' + qdrant)
    return config, hashes


def stored_vectors(base: list[str]) -> int:
    """Check the public readiness boundary instead of a removed local SQL cache."""
    output = subprocess.run(base + ['status'], check=True, capture_output=True)
    status = json.loads(output.stdout)
    if not isinstance(status, dict) or status.get('ready') is not True:
        raise ValueError('benchmark gold is not certified current')
    count = status.get('embeddings')
    if type(count) is not int or count < 0:
        raise ValueError('invalid vector count in status')
    return count


def canonical_content(base: list[str], message_id: str) -> str:
    # Let Mash decode packed silver rather than duplicating its codec here.
    output = subprocess.run(base + ['view', message_id, '--context', '0'],
                            check=True, capture_output=True)
    value = json.loads(output.stdout)
    messages = value.get('messages') if isinstance(value, dict) else None
    if not isinstance(messages, list) or len(messages) != 1 or not isinstance(messages[0], dict):
        raise ValueError('expected one canonical message')
    content = messages[0].get('content')
    if not isinstance(content, str):
        raise ValueError('missing canonical message content')
    return content


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
    qdrant = COST.qdrant_configuration(args.qdrant_url, args.qdrant_grpc_url)
    output.mkdir()
    binary_hash = digest(binary)
    results = []
    for events, sources in CASES:
        root = output / f'events-{events}-sources-{sources}'
        root.mkdir()
        config, hashes = fixture(root, events, sources, qdrant)
        database = root / 'index.sqlite3'
        base = [str(binary), '--config', str(config), '--db', str(database),
                '--models-dir', str(models)]
        measurement = measure(base + ['index'], binary, root)
        indexed = json.loads((root / 'stdout.json').read_text())
        with COST.connection(database) as connection:
            identities = connection.execute(
                "SELECT id,role FROM objects WHERE origin='provider' AND object_role='entry' ORDER BY id"
            ).fetchall()
            conversations = connection.execute(
                "SELECT COUNT(*) FROM objects WHERE origin='provider' AND object_role='container'"
            ).fetchone()[0]
        rows = [(message_id, role, canonical_content(base, message_id))
                for message_id, role in identities]
        vectors = stored_vectors(base)
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
            'Qdrant server RAM and CPU are outside the measured CLI process',
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
    parser.add_argument('--qdrant-url', required=True)
    parser.add_argument('--qdrant-grpc-url', required=True)
    run(parser.parse_args())


if __name__ == '__main__':
    main()
