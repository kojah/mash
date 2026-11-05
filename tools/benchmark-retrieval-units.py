#!/usr/bin/env python3
"""Compare local views units on a private copy of already certified silver.

No provider reads or remote inference. Output must be outside the repository.
Requires disposable local Qdrant. Measures build/process-cold search costs and
SQLite silver storage, excluding Qdrant server memory/disk and semantic quality.
"""
import argparse
import json
from pathlib import Path
import sqlite3

from importlib.util import module_from_spec, spec_from_file_location

SPEC = spec_from_file_location('views_cost', Path(__file__).with_name('benchmark-views.py'))
if SPEC is None or SPEC.loader is None:
    raise ImportError('benchmark-views.py could not be loaded')
COST = module_from_spec(SPEC)
SPEC.loader.exec_module(COST)


def run(args):
    binary = args.binary.resolve(strict=True)
    models = args.models.resolve(strict=True)
    source = args.source_db.resolve(strict=True)
    output = args.output.resolve()
    qdrant = COST.qdrant_configuration(args.qdrant_url, args.qdrant_grpc_url)
    if output.is_relative_to(Path(__file__).resolve().parent.parent):
        raise ValueError('private output must be outside the repository')
    output.mkdir(mode=0o700)
    database = output / 'index.sqlite3'
    with sqlite3.connect(source.as_uri() + '?mode=ro', uri=True) as original:
        with sqlite3.connect(database) as copied:
            original.backup(copied)
    canonical = COST.silver_digest(database)
    base = [str(binary), '--db', str(database), '--models-dir', str(models)]
    stages = []
    for number, unit in enumerate(('message', 'parent-and-message', 'message')):
        config = output / f'{unit}.toml'
        config.write_text('version=1\n[providers]\n[views.build]\n' +
                          f'retrieval_unit="{unit}"\n[views.query]\nintent="exact-mention"\n' + qdrant)
        command = base + ['--config', str(config)]
        result, measurement = COST.measure(command + ['index', '--rebuild-local-views'],
                                            binary, output / f'build-{number}-{unit}')
        if COST.silver_digest(database) != canonical or result['scanned_files']:
            raise ValueError('unit replacement changed silver or read provider files')
        searches = []
        for intent, ranking in (('exact-mention', None), ('related-discussion', 'fusion')):
            for query in COST.QUERIES:
                result_search, timing = COST.measure(command + [
                    'search', query, '--local', '--intent', intent
                ] + (['--ranking', ranking] if ranking else []), binary,
                    output / f'search-{number}-{len(searches)}')
                searches.append({'intent': intent, 'query': query, 'measurement': timing,
                                 'response': result_search})
        if COST.silver_digest(database) != canonical:
            raise ValueError('search changed canonical history')
        stages.append({'unit': unit, 'build': result, 'measurement': measurement,
                       'storage': COST.storage(database), 'searches': searches})
    capture = {'scope': 'copied-certified-silver-local-retrieval-units-v1',
               'binary_sha256': COST.sha(binary), 'script_sha256': COST.sha(Path(__file__)),
               'model_marker_sha256': COST.sha(models / 'installed.json'),
               'silver_sha256': canonical, 'stages': stages,
               'limitations': ['one Linux node, copied corpus; no provider ingestion',
                               'fresh CLI processes; filesystem/model pages may be cached',
                               'one query trial per stage; no p95 claim or semantic labels',
                               '20ms sampled RSS/PSS maxima are lower bounds',
                               'wait4 maxrss can include launcher peaks',
                               'no shared backend, resident worker, edits/deletes or quality evaluation']}
    COST.write_json(output / 'capture.json', capture)
    print(json.dumps([{'unit': stage['unit'], 'seconds': stage['measurement']['wall_seconds'],
                       'storage': stage['storage']} for stage in stages]), flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--models', type=Path, required=True)
    parser.add_argument('--source-db', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--qdrant-url', required=True)
    parser.add_argument('--qdrant-grpc-url', required=True)
    run(parser.parse_args())
