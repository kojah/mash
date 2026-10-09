"""Fixed synthetic release-worker tracing control, not a latency acceptance gate."""
import argparse
import hashlib
import json
import math
import os
import platform
import sqlite3
import statistics
import subprocess
import time
from pathlib import Path

SUBJECTS = [
    'SQLite search indexes', 'PostgreSQL connection pools', 'Redis vector filters',
    'Qdrant snapshot publication', 'Rust async inference', 'MCP progress notifications',
    'Claude transcript ingestion', 'Codex thread identities', 'Voyage embedding models',
    'local search workers',
]
ACTIONS = ['configure', 'test', 'debug', 'monitor', 'deploy']
QUERIES = [f'How should we {action} {subject}?' for subject in SUBJECTS for action in ACTIONS]
CANARY = 'synthetic-history-privacy-canary'
STAGES = {
    'search', 'local_search', 'load_search_models', 'embed', 'hybrid_search',
    'lexical_candidates', 'load_vectors', 'score_vectors', 'combined_search',
    'prepare_search', 'retrieve_and_rank', 'local_candidates', 'shared_http_request',
    'rerank', 'rerank_inference',
}


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def summary(values):
    ordered = sorted(values)
    return {'samples': len(values), 'median_ms': statistics.median(values),
            'p95_ms': ordered[math.ceil(.95 * len(values)) - 1],
            'minimum_ms': ordered[0], 'maximum_ms': ordered[-1]}


def fixture(root):
    history = root / 'claude'
    history.mkdir()
    hashes = {}
    for index, query in enumerate(QUERIES):
        path = history / f'control-{index}.jsonl'
        session = f'tracing-control-{index}'
        user = {'type': 'user', 'sessionId': session, 'uuid': 'user',
                'message': {'role': 'user', 'content': query + ' ' + CANARY}}
        assistant = {'type': 'assistant', 'sessionId': session, 'uuid': 'assistant',
                     'parentUuid': 'user', 'message': {'role': 'assistant', 'content':
                     'Use explicit configuration and reproducible tests. Check observability, '
                     'ownership, recovery and relevant search results before deployment.'}}
        path.write_text(json.dumps(user) + '\n' + json.dumps(assistant) + '\n')
        hashes[path.name] = digest(path)
    config = root / 'config.toml'
    config.write_text('version = 1\n[index]\nall_history = true\n'
                      '[providers.claude-code]\nroots = ' + json.dumps([str(history)]) + '\n')
    return config, hashes


def diagnostics(stderr, traced, forbidden):
    if any(value in stderr for value in forbidden):
        raise ValueError('external content leaked to diagnostic stderr')
    if not traced:
        if stderr:
            raise ValueError('untraced search emitted stderr')
        return None
    events = [json.loads(line) for line in stderr.splitlines()]
    replies = [event['fields'] for event in events
               if event.get('fields', {}).get('message') == 'worker reply']
    stages = [event['fields'] for event in events
              if event.get('fields', {}).get('message') == 'worker stage']
    if len(replies) != 1 or not 1 <= len(stages) <= 15:
        raise ValueError('missing or unbounded worker diagnostics')
    reply = replies[0]
    identities = {stage['stage'] for stage in stages}
    if len(identities) != len(stages) or not identities <= STAGES:
        raise ValueError('unexpected or repeated diagnostic stage')
    for stage in stages:
        if (stage['trace_id'] != reply['trace_id']
                or stage['worker_process_id'] != reply['worker_process_id']
                or stage['calls'] < 1 or stage['total_us'] < stage['max_us']):
            raise ValueError('invalid diagnostic correlation or counters')
    return {'reply': reply, 'stages': stages, 'stderr_bytes': len(stderr.encode())}


def run(args):
    if platform.system() != 'Linux':
        raise ValueError('this fixed control requires Linux cached workers')
    binary = args.binary.resolve(strict=True)
    models = args.models.resolve(strict=True)
    root = args.output.resolve()
    root.mkdir()  # Never overwrite a previous capture.
    binary_pin = digest(binary)
    model_pin = digest(models / 'installed.json')
    model_spec = json.loads((models / 'installed.json').read_text())
    asset_pins = {}
    for entry in model_spec['files']:
        path = (models / entry['path']).resolve(strict=True)
        if not path.is_relative_to(models) or path.stat().st_size != entry['size']:
            raise ValueError('model asset identity differs from installed metadata')
        asset_pins[entry['path']] = digest(path)
    config, inputs = fixture(root)
    database = root / 'index.sqlite3'
    base = [str(binary), '--config', str(config), '--db', str(database),
            '--models-dir', str(models)]
    environment = dict(os.environ)
    environment.pop('MASH_TRACE', None)
    environment['TMPDIR'] = str(args.worker_temp_dir.resolve(strict=True))
    index = subprocess.run(base + ['index'], env=environment, capture_output=True,
                           text=True, timeout=180, check=True)
    (root / 'index.stdout.json').write_text(index.stdout)
    (root / 'index.stderr.jsonl').write_text(index.stderr)
    with sqlite3.connect(database.as_uri() + '?mode=ro', uri=True) as connection:
        counts = {table: connection.execute(f'SELECT count(*) FROM {table}').fetchone()[0]
                  for table in ['conversations', 'messages', 'message_embeddings']}
    if counts['conversations'] != 50 or counts['messages'] != 100 or counts['message_embeddings'] != 100:
        raise ValueError('fixture ingestion counts differ from frozen workload')
    database_pin = digest(database)
    forbidden = [CANARY, str(root), str(models), *QUERIES]
    samples, worker_ids, trace_ids = [], set(), set()

    def search(query, unit, traced):
        env = {**environment, 'MASH_TRACE': '1'} if traced else environment
        start = time.perf_counter_ns()
        result = subprocess.run(base + ['search', query, '--local', '--unit', unit, '--limit', '5'],
                                env=env, capture_output=True, text=True, timeout=30, check=True)
        wall_ms = (time.perf_counter_ns() - start) / 1_000_000
        output = json.loads(result.stdout)
        if not output['results'] or output['realized_mode'] != 'hybrid' or output['shared']:
            raise ValueError('search did not yield expected ready local results')
        diagnostic = diagnostics(result.stderr, traced, forbidden)
        if diagnostic:
            reply = diagnostic['reply']
            worker_ids.add(reply['worker_process_id'])
            if reply['trace_id'] in trace_ids:
                raise ValueError('trace identity repeated')
            trace_ids.add(reply['trace_id'])
        return output, {'wall_ms': wall_ms, 'diagnostics': diagnostic}

    # Warmup excludes model startup; both units run in the same isolated worker.
    warmup = [search(QUERIES[0], unit, True)[1] for unit in ['chats', 'messages']]
    for index, query in enumerate(QUERIES):
        units = ['chats', 'messages'] if index % 2 == 0 else ['messages', 'chats']
        for unit in units:
            modes = [False, True] if index % 2 == 0 else [True, False]
            pair = {}
            outputs = []
            for traced in modes:
                output, measurement = search(query, unit, traced)
                outputs.append(output)
                pair['on' if traced else 'off'] = measurement
            if outputs[0] != outputs[1]:
                raise ValueError('tracing changed complete ranked output')
            samples.append({'query_index': index, 'unit': unit, 'order': modes,
                            'result_sha256': hashlib.sha256(json.dumps(outputs[0], sort_keys=True).encode()).hexdigest(),
                            'load_average': os.getloadavg(), **pair})
    if len(worker_ids) != 1 or digest(binary) != binary_pin or digest(models / 'installed.json') != model_pin:
        raise ValueError('worker/binary/model metadata changed during control')
    if digest(database) != database_pin:
        raise ValueError('search modified the indexed database')
    if any(digest(models / path) != pin for path, pin in asset_pins.items()):
        raise ValueError('model assets changed during control')
    summaries = {}
    for unit in ['chats', 'messages']:
        selected = [row for row in samples if row['unit'] == unit]
        summaries[unit] = {mode: summary([row[mode]['wall_ms'] for row in selected])
                           for mode in ['off', 'on']}
        summaries[unit]['paired_difference_ms'] = summary([
            row['on']['wall_ms'] - row['off']['wall_ms'] for row in selected])
    capture = {'scope': 'synthetic-release-local-worker-tracing-v1',
               'binary_sha256': binary_pin, 'script_sha256': digest(Path(__file__)),
               'model_metadata_sha256': model_pin, 'database_sha256': database_pin,
               'model_assets_sha256': asset_pins,
               'models': {key: model_spec[key] for key in ['embedding_model', 'reranker_model', 'embedding_dimensions']},
               'config_sha256': digest(config), 'input_sha256': inputs,
               'platform': platform.platform(), 'python': platform.python_version(),
               'logical_cpus': os.cpu_count(), 'queries': QUERIES, 'counts': counts,
               'worker_pid': next(iter(worker_ids)), 'warmup': warmup,
               'samples': samples, 'summary': summaries,
               'limitations': ['generated 50-thread local control, not real-corpus relevance or latency acceptance',
                               'CLI wall times include process startup/IPC/JSON; trace stages are inclusive',
                               'counterbalanced paired modes on one warm worker; no independent workers/host replication',
                               'no shared network, native Mac Core ML, fleet or confidence-bound overhead claim',
                               'loaded models and other worker caches retained; production search behavior unchanged']}
    (root / 'capture.json').write_text(json.dumps(capture, indent=2) + '\n')
    print(json.dumps({'worker_pid': capture['worker_pid'], 'summary': summaries}), flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--models', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--worker-temp-dir', required=True, type=Path)
    run(parser.parse_args())
