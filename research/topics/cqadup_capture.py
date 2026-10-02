"""Capture official BEIR programmers members with bounded conditional range reads."""
import argparse
import hashlib
import io
import json
from pathlib import Path
import urllib.request
import zipfile


URL = 'https://public.ukp.informatik.tu-darmstadt.de/thakur/BEIR/datasets/cqadupstack.zip'
SIZE = 5343728040
ETAG = '"607ee440-13e82d1a8"'


class Remote(io.RawIOBase):
    def __init__(self, fetch=None):
        self.position = 0
        self.transferred = 0
        self.requests = 0
        self.fetch = fetch or self.request

    def seekable(self):
        return True

    def readable(self):
        return True

    def tell(self):
        return self.position

    def seek(self, offset, whence=0):
        target = offset + (self.position if whence == 1 else SIZE if whence == 2 else 0)
        if whence not in (0, 1, 2) or target < 0:
            raise ValueError('invalid archive seek')
        self.position = target
        return target

    def read(self, size=-1):
        if size < 0:
            size = SIZE - self.position
        size = min(size, max(0, SIZE - self.position))
        if size > 8 * 1024 * 1024 or self.transferred + size > 384 * 1024 * 1024:
            raise ValueError('bounded range/capture byte budget exceeded')
        if not size:
            return b''
        start, end = self.position, self.position + size - 1
        raw = self.fetch(start, end)
        if len(raw) != size:
            raise ValueError('short archive range')
        self.position += size
        self.transferred += size
        self.requests += 1
        return raw

    @staticmethod
    def request(start, end):
        request = urllib.request.Request(URL, headers={'Range': f'bytes={start}-{end}', 'If-Match': ETAG})
        with urllib.request.urlopen(request, timeout=30) as response:
            if (response.status != 206 or response.headers.get('Content-Range') != f'bytes {start}-{end}/{SIZE}'
                    or response.headers.get('ETag') != ETAG):
                raise ValueError('immutable exact HTTP range required; refusing whole archive')
            raw = response.read(end - start + 2)
            if len(raw) != end - start + 1:
                raise ValueError('range response length mismatch')
            return raw


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-directory', type=Path, required=True)
    parser.add_argument('--list-only', action='store_true')
    args = parser.parse_args()
    if args.output_directory.exists() or args.output_directory.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git capture directory required')
    args.output_directory.mkdir()
    remote = Remote()
    with zipfile.ZipFile(remote) as archive:
        candidates = [i for i in archive.infolist() if '/programmers/' in i.filename and not i.is_dir()]
        members = [{'name': i.filename, 'size': i.file_size, 'compressed_size': i.compress_size,
                    'crc32': i.CRC} for i in candidates]
        if args.list_only:
            result = {'url': URL, 'size': SIZE, 'etag': ETAG, 'members': members,
                      'bytes_transferred': remote.transferred, 'requests': remote.requests}
            (args.output_directory / 'members.json').write_text(json.dumps(result, sort_keys=True))
            print(json.dumps(result))
            return
        suffixes = {'corpus.jsonl': 'corpus.jsonl', 'queries.jsonl': 'queries.jsonl', 'qrels/test.tsv': 'qrels.tsv'}
        selected = {}
        for suffix, output in suffixes.items():
            matches = [i for i in candidates if i.filename.endswith('/programmers/' + suffix)]
            if len(matches) != 1:
                raise ValueError('exact unique programmers corpus/query/qrels members required')
            selected[output] = matches[0]
        if sum(i.file_size for i in selected.values()) > 1024 * 1024 * 1024:
            raise ValueError('complete selected-source capture cap exceeded; do not truncate')
        files = {}
        for name, info in selected.items():
            sha, total = hashlib.sha256(), 0
            with archive.open(info) as source, (args.output_directory / name).open('xb') as output:
                while chunk := source.read(1024 * 1024):
                    output.write(chunk)
                    sha.update(chunk)
                    total += len(chunk)
                    if total // (16 * 1024 * 1024) != (total - len(chunk)) // (16 * 1024 * 1024):
                        print(json.dumps({'stage': 'member-progress', 'name': name, 'bytes': total}), flush=True)
            if total != info.file_size:
                raise ValueError('complete archive member required')
            files[name] = {'archive_name': info.filename, 'size': total, 'sha256': sha.hexdigest(),
                           'crc32': info.CRC, 'compressed_size': info.compress_size}
            print(json.dumps({'stage': 'member-complete', 'name': name, 'bytes': total}), flush=True)
    provenance = {'version': 1, 'scope': 'official-BEIR-programmers-range-capture-no-results',
                  'url': URL, 'archive_size': SIZE, 'etag': ETAG,
                  'official_archive_md5': '4e41456d7df8ee7760a7f866133bda78', 'full_archive_md5_verified': False,
                  'member_crc_verified': True, 'files': files, 'bytes_transferred': remote.transferred,
                  'requests': remote.requests, 'capture_code_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
    (args.output_directory / 'capture.json').write_text(json.dumps(provenance, sort_keys=True))
    print(json.dumps({'stage': 'complete', 'members': len(files), 'bytes_transferred': remote.transferred}))


if __name__ == '__main__':
    main()
