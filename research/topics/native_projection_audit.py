"""Bounded current-native corroboration of frozen Claude empty projections, not reindexing."""
import argparse
from collections import Counter, defaultdict
import json
from pathlib import Path
import sqlite3
import time

import blake3

from external_context import compare
from reference_review import digest
from stored_frequency import CAPTURE


CONTEXT = {'tool_use', 'toolCall', 'tool_result', 'thinking', 'event_msg', 'item_completed',
           'task_started', 'task_complete', 'unknown', 'system'}
TEXT = {'text', 'input_text', 'output_text'}


def stable_message(thread, key):
    # d62a597 normalizes preliminary path IDs to native thread/UUID identities.
    return blake3.blake3(b'claude-message\0' + thread.encode() + b'\0' + key.encode()).hexdigest()[:24]


def flatten(value, depth=0):
    if depth > 64:
        raise ValueError('native content recursion exceeds cap')
    if isinstance(value, str):
        return value
    if isinstance(value, list):
        parts = [part for item in value if (part := flatten(item, depth + 1)) is not None]
        return '\n'.join(parts) if parts else None
    if not isinstance(value, dict):
        return None
    if isinstance(value.get('text'), str):
        return value['text']
    if isinstance(value.get('content'), str):
        return value['content']
    if value.get('type') == 'thinking' and isinstance(value.get('thinking'), str):
        return '[Thinking] ' + value['thinking']
    if value.get('type') == 'tool_result':
        return flatten(value.get('content'), depth + 1)
    # Unknown canonical reconstructions are explicitly unsupported, never guessed.
    return None


def category(value):
    blocks = value if isinstance(value, list) else [value]
    if all(isinstance(v, str) for v in blocks):
        return 'native-text'
    if not all(isinstance(v, dict) for v in blocks):
        return 'native-unclassified-shape'
    kinds = {v.get('type') for v in blocks}
    if kinds == {'tool_result'}:
        return 'native-tool-result-only'
    if kinds and kinds <= CONTEXT:
        return 'native-context-only'
    if kinds and kinds <= TEXT:
        return 'native-text-blocks-only'
    return 'native-mixed-or-unknown-blocks'


def experiment(database, capture):
    started = time.perf_counter()
    raw = capture.read_bytes()
    if digest(raw) != CAPTURE:
        raise ValueError('original frozen selected-source capture required')
    source = json.loads(raw)
    pending = defaultdict(dict)
    with sqlite3.connect(database.resolve().as_uri() + '?mode=ro', uri=True) as connection:
        connection.execute('BEGIN')
        for identity in source['records']:
            for key, content, path in connection.execute(
                "SELECT m.id,m.content,c.source_path FROM messages m JOIN conversations c ON c.id=m.conversation_id "
                "WHERE c.id=? AND c.provider='claude-code' AND m.role='user' AND m.search_projection=''", (identity,)):
                pending[path][key] = (identity, digest(content.encode()))
    matches, files, source_bytes = defaultdict(set), {}, 0
    for name, targets in sorted(pending.items()):
        path = Path(name)
        matched = defaultdict(set)
        if not path.is_file():
            files[digest(name.encode())] = {'status': 'missing-current-source', 'targets': len(targets)}
            continue
        before = path.stat()
        malformed = unsupported = lines = 0
        hasher = blake3.blake3()
        with path.open('rb') as stream:
            for line in stream:
                source_bytes += len(line)
                lines += 1
                if len(line) > 64 * 1024**2 or source_bytes > 1024**3 or lines > 1_000_000:
                    raise ValueError('native audit exceeds bounds; no cap increase or partial capture')
                hasher.update(line)
                if not line.strip():
                    continue
                try:
                    record = json.loads(line)
                except (json.JSONDecodeError, UnicodeDecodeError, RecursionError):
                    malformed += 1
                    continue
                if not isinstance(record, dict) or record.get('type') != 'user' or not isinstance(record.get('uuid'), str):
                    continue
                candidate_keys = [stable_message(thread, record['uuid']) for thread in {t for t, _ in targets.values()}]
                keys = [key for key in candidate_keys if key in targets]
                if not keys:
                    continue
                message = record.get('message')
                if not isinstance(message, dict) or message.get('role', 'user') != 'user':
                    unsupported += 1
                    continue
                value = message.get('content')
                try:
                    canonical = flatten(value)
                except ValueError:
                    unsupported += 1
                    continue
                exact = [key for key in keys if canonical is not None and digest(canonical.encode()) == targets[key][1]]
                if not exact:
                    unsupported += 1
                    continue
                for key in exact:
                    matched[key].add(category(value))
        after = path.stat()
        stable = (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns) == (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns)
        files[digest(name.encode())] = {'status': 'stable-current-source' if stable else 'changed-during-audit',
                                      'targets': len(targets), 'bytes': before.st_size, 'lines': lines,
                                      'current_source_blake3': hasher.hexdigest(), 'malformed_lines': malformed,
                                      'unsupported_or_content_mismatch': unsupported}
        if stable:
            for key, kinds in matched.items():
                matches[key].update(kinds)
    counts = Counter()
    for targets in pending.values():
        for key in targets:
            kinds = matches.get(key, set())
            counts[next(iter(kinds)) if len(kinds) == 1 else 'ambiguous-native-category' if kinds else 'not-corroborated'] += 1
    return {'version': 1, 'scope': 'current-native-corroboration-not-historical-complete-parser-equivalence',
            'identity_contract': 'd62a597-native-claude-message-thread-uuid',
            'targets_sha256': digest(json.dumps({name: dict(sorted(values.items())) for name, values in sorted(pending.items())}, sort_keys=True).encode()),
            'selection_sha256': CAPTURE, 'source_bytes': source_bytes, 'categories': dict(counts), 'files': files,
            'code_pins': {name: digest(Path(__file__).with_name(name).read_bytes()) for name in ('native_projection_audit.py', 'uv.lock')},
            'timings': {'total_ms': (time.perf_counter() - started) * 1000}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('database', 'capture', 'output'):
        parser.add_argument('--' + name, required=True, type=Path)
    parser.add_argument('--compare', type=Path)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git output required')
    result = experiment(args.database, args.capture)
    if args.compare:
        compare(json.loads(args.compare.read_text()), result)
    with args.output.open('x') as output:
        json.dump(result, output, sort_keys=True, allow_nan=False)
    print(json.dumps({'categories': result['categories'], 'files': len(result['files']), 'source_bytes': result['source_bytes']}))


if __name__ == '__main__':
    main()
