"""Capture pinned author QMSum training data only, without models or held-out files."""
import argparse
import json
from pathlib import Path
import urllib.request

from tiage_capture import verify


COMMIT = '83d7768c1f2b4dfeb091385d3dc7e239b8e5bb7e'
TREE = '159791fb5fae5ef625bd0ce4d300fd3d53aca9c8'
REPOSITORY = 'https://github.com/Yale-LILY/QMSum'
FILES = {'README.md': ('3c0a8f9c0db9f418414bb3b399954e06e85aac06', 5959),
         'LICENSE': ('3260fade8bfc520cc284adbb4ef2d397c8f7c23e', 1066),
         'data/ALL/jsonl/train.jsonl': ('612fb8ca3e2810c85af1821691b38b2961347528', 11940700)}


def capture(destination):
    if destination.exists() or destination.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git source capture required')
    destination.mkdir(parents=True);records = {}
    for path, (blob, size) in FILES.items():
        url = f'https://raw.githubusercontent.com/Yale-LILY/QMSum/{COMMIT}/{path}'
        with urllib.request.urlopen(url, timeout=30) as response:raw = response.read(size+1)
        pin = verify(raw, blob, size);target = destination/path;target.parent.mkdir(parents=True, exist_ok=True)
        with target.open('xb') as f:f.write(raw)
        records[path] = {'git_blob': blob, 'sha256': pin, 'bytes': size, 'url': url}
        print(json.dumps({'captured': path, 'sha256': pin}), flush=True)
    with (destination/'capture.json').open('x') as f:
        json.dump({'schema_version': 1, 'repository': REPOSITORY, 'commit': COMMIT, 'tree': TREE,
                   'files': records, 'scope': 'author-training-topic-span-source-preflight',
                   'validation_test_downloaded': False, 'inference_calls': 0}, f, indent=2)


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__);p.add_argument('--output-directory', required=True, type=Path)
    capture(p.parse_args().output_directory)
