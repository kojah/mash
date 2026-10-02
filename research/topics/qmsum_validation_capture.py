"""Separate pinned author-validation capture; shared typed original parser."""
import argparse
from dataclasses import dataclass
import hashlib
import json
from pathlib import Path
import urllib.request

from qmsum import Meeting, fields, parse
from qmsum_capture import COMMIT, TREE, REPOSITORY, FILES as TRAINING_FILES
from stage_parity import decode
from tiage_capture import verify


SOURCE = 'data/ALL/jsonl/val.jsonl'
FILES = {name: TRAINING_FILES[name] for name in ['README.md','LICENSE']} | {
    SOURCE: ('f6d2b619c21fea5d1b6e35d697af483dd2802d24',2714748)}
SCOPE = 'author-validation-guidance-source-preflight'


def protocol_sha256() -> str:
    return hashlib.sha256(Path(__file__).with_name('GUIDANCE_VALIDATION_PROTOCOL.md').read_bytes()).hexdigest()


def url(path: str) -> str:
    return f'https://raw.githubusercontent.com/Yale-LILY/QMSum/{COMMIT}/{path}'


def capture(destination: Path) -> None:
    if destination.exists() or destination.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git validation capture required')
    destination.mkdir(parents=True); records = {}; protocol = protocol_sha256()
    for path,(blob,size) in FILES.items():
        with urllib.request.urlopen(url(path),timeout=30) as response: raw = response.read(size+1)
        pin = verify(raw,blob,size); target = destination/path; target.parent.mkdir(parents=True,exist_ok=True)
        with target.open('xb') as f: f.write(raw)
        records[path] = {'git_blob':blob,'sha256':pin,'bytes':size,'url':url(path)}
        print(json.dumps({'captured':path,'sha256':pin}),flush=True)
    if protocol_sha256() != protocol: raise ValueError('validation protocol changed during capture')
    with (destination/'capture.json').open('x') as f:
        json.dump({'schema_version':1,'repository':REPOSITORY,'commit':COMMIT,'tree':TREE,'files':records,
            'scope':SCOPE,'test_downloaded':False,'inference_calls':0,'protocol_sha256':protocol},f,indent=2,allow_nan=False)


@dataclass(frozen=True)
class CapturedFile:
    git_blob: str
    sha256: str
    bytes: int
    url: str


def file_metadata(value: dict) -> CapturedFile:
    fields(value,['git_blob','sha256','bytes','url'])
    if (any(type(value[k]) is not str for k in ['git_blob','sha256','url']) or type(value['bytes']) is not int):
        raise ValueError('typed original file metadata required')
    return CapturedFile(**value)


@dataclass(frozen=True)
class ValidationSource:
    capture_sha256: str
    source_sha256: str
    protocol_sha256: str
    meetings: tuple[Meeting, ...]


@dataclass(frozen=True)
class SplitCheck:
    training_meetings: int
    validation_meetings: int
    training_unique_turn_texts: int
    validation_unique_turn_texts: int
    shared_unique_turn_texts: int
    shared_validation_turn_occurrences: int


def transcript_sha256(meeting: Meeting) -> str:
    raw = json.dumps([turn.text for turn in meeting.turns],ensure_ascii=False,separators=(',',':')).encode()
    return hashlib.sha256(raw).hexdigest()


def check_disjoint(source: ValidationSource, training: tuple[Meeting, ...]) -> SplitCheck:
    train = {transcript_sha256(meeting) for meeting in training}
    validation = [transcript_sha256(meeting) for meeting in source.meetings]
    if len(set(validation)) != len(validation) or train.intersection(validation):
        raise ValueError('duplicate validation or cross-split complete transcript; no source dropped')
    train_texts = {turn.text for meeting in training for turn in meeting.turns}
    val_texts = {turn.text for meeting in source.meetings for turn in meeting.turns}
    return SplitCheck(len(training),len(source.meetings),len(train_texts),len(val_texts),len(train_texts & val_texts),
        sum(turn.text in train_texts for meeting in source.meetings for turn in meeting.turns))


def load(root: Path) -> ValidationSource:
    if root.is_symlink() or not root.is_dir(): raise ValueError('regular validation source directory required')
    raw = (root/'capture.json').read_bytes(); metadata = decode(raw)
    fields(metadata,['schema_version','repository','commit','tree','files','scope','test_downloaded','inference_calls','protocol_sha256'])
    if (type(metadata['schema_version']) is not int or metadata['schema_version'] != 1 or metadata['repository'] != REPOSITORY
            or metadata['commit'] != COMMIT or metadata['tree'] != TREE or metadata['scope'] != SCOPE
            or metadata['test_downloaded'] is not False or type(metadata['inference_calls']) is not int or metadata['inference_calls'] != 0
            or metadata['protocol_sha256'] != protocol_sha256() or type(metadata['files']) is not dict or set(metadata['files']) != set(FILES)):
        raise ValueError('pinned separate validation source contract required')
    present = {str(p.relative_to(root)) for p in root.rglob('*') if p.is_file()}
    if present != set(FILES) | {'capture.json'} or any(p.is_symlink() for p in root.rglob('*')):
        raise ValueError('unexpected files or symlinks in validation source')
    source = None; content = None
    for path,(blob,size) in FILES.items():
        payload = (root/path).read_bytes(); pin = verify(payload,blob,size)
        if file_metadata(metadata['files'][path]) != CapturedFile(blob,pin,size,url(path)):
            raise ValueError('validation file manifest drift')
        if path == SOURCE: source = pin; content = payload
    if source is None or content is None: raise ValueError('complete original validation transcript required')
    return ValidationSource(hashlib.sha256(raw).hexdigest(),source,metadata['protocol_sha256'],parse(content))


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__); p.add_argument('--output-directory',type=Path,required=True)
    capture(p.parse_args().output_directory)
