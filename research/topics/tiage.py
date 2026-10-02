"""Strict original TIAGE training turns; annotations are boundary gold, not features."""
import argparse
from collections import Counter
from dataclasses import dataclass
from enum import StrEnum
import hashlib
import json
from pathlib import Path
import platform
import re

from stage_parity import decode
from tiage_capture import COMMIT,FILES,REPOSITORY,verify


class Shift(StrEnum):
    START = '-1'
    SAME = '0'
    CHANGE = '1'


@dataclass(frozen=True)
class Turn:
    ordinal: int
    text: str
    shift: Shift


@dataclass(frozen=True)
class Dialogue:
    id: str
    turns: tuple[Turn,...]
    source_sha256: str


def parse(raw):
    data=decode(raw)
    if type(data) is not dict or not data:raise ValueError('nonempty original dialogue object required')
    result=[]
    for identity,rows in data.items():
        if type(identity) is not str or re.fullmatch('[1-9][0-9]{0,8}',identity) is None:
            raise ValueError('original numeric string dialogue identity required')
        if type(rows) is not list or len(rows)<2:raise ValueError('complete nontrivial dialogue required')
        turns=[]
        for ordinal,row in enumerate(rows):
            if type(row) is not list or len(row)!=2 or any(type(v) is not str for v in row):
                raise ValueError('original [text, string label] row required')
            text,label=row
            if not text.strip():raise ValueError('empty source turn; no silent exclusion')
            shift=Shift(label)
            if (shift==Shift.START)!=(ordinal==0):raise ValueError('root sentinel only at original ordinal zero')
            turns.append(Turn(ordinal,text,shift))
        pin=hashlib.sha256(json.dumps(rows,ensure_ascii=False,separators=(',',':')).encode()).hexdigest()
        result.append(Dialogue(identity,tuple(turns),pin))
    return tuple(result)


def load(root):
    raw=(root/'capture.json').read_bytes();capture=decode(raw)
    if (type(capture) is not dict or set(capture)!={'schema_version','repository','commit','files','scope',
            'development_test_weak_supervision_downloaded','inference_calls'}
            or type(capture.get('files')) is not dict):raise ValueError('invalid capture schema')
    if (capture.get('schema_version')!=1 or type(capture.get('schema_version')) is not int
            or capture.get('commit')!=COMMIT or capture.get('repository')!=REPOSITORY
            or capture.get('development_test_weak_supervision_downloaded') is not False
            or capture.get('scope')!='author-annotated-training-only-structural-preflight'
            or type(capture.get('inference_calls')) is not int or capture['inference_calls']!=0
            or set(capture.get('files',{}))!=set(FILES)):
        raise ValueError('pinned training-only author capture required')
    present={str(p.relative_to(root)) for p in root.rglob('*') if p.is_file()}
    if present!=set(FILES)|{'capture.json'}:raise ValueError('unexpected captured files; reserved data must remain absent')
    source_bytes=None
    for path,(blob,size) in FILES.items():
        target=root/path
        if target.is_symlink():raise ValueError('regular captured source files required')
        content=target.read_bytes();pin=verify(content,blob,size);record=capture['files'][path]
        if record!={'git_blob':blob,'sha256':pin,'bytes':size,
                    'url':f'https://raw.githubusercontent.com/HuiyuanXie/tiage/{COMMIT}/{path}'}:
            raise ValueError('author source provenance drift')
        if path=='data/personachat/anno/train/anno_train.json':source_bytes=content
    if source_bytes is None:raise ValueError('missing original training source')
    dialogues=parse(source_bytes)
    if {d.id for d in dialogues}!={str(i) for i in range(1,301)}:raise ValueError('complete pinned training identities required')
    return hashlib.sha256(raw).hexdigest(),hashlib.sha256(source_bytes).hexdigest(),dialogues


def preflight(root):
    capture_pin,source_pin,dialogues=load(root);labels=Counter(t.shift for d in dialogues for t in d.turns)
    texts=Counter(t.text for d in dialogues for t in d.turns)
    ledger=[{'id':d.id,'source_sha256':d.source_sha256,'turns':[
        {'ordinal':t.ordinal,'label':t.shift,'text_sha256':hashlib.sha256(t.text.encode()).hexdigest(),
         'characters':len(t.text),'utf8_bytes':len(t.text.encode())} for t in d.turns]} for d in dialogues]
    return {'schema_version':1,'scope':'training-source-label-preflight-no-inference-or-scoring',
            'python':platform.python_version(),
            'code_pins':{name:hashlib.sha256(Path(__file__).with_name(name).read_bytes()).hexdigest()
                         for name in ['tiage.py','tiage_capture.py','stage_parity.py']},
            'commit':COMMIT,'capture_sha256':capture_pin,'source_sha256':source_pin,
            'dialogues':len(dialogues),'turns':sum(labels.values()),'labels':dict(labels),
            'scorable_original_boundaries':labels[Shift.SAME]+labels[Shift.CHANGE],
            'exact_unique_turn_texts':len(texts),'repeated_text_occurrences':sum(n-1 for n in texts.values()),
            'minimum_turns':min(len(d.turns) for d in dialogues),'maximum_turns':max(len(d.turns) for d in dialogues),
            'ledger':ledger,'inference_calls':0,'development_test_scored':False,
            'limitations':['label at response denotes change from preceding turn, root sentinel is not a negative',
                           'no user/assistant roles or cross-episode topic identities supplied',
                           'human annotation concerns open-domain PersonaChat, not coding-topic ancestry']}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--source',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True);p.add_argument('--compare',type=Path);args=p.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git output required')
    result=json.loads(json.dumps(preflight(args.source),allow_nan=False))
    if args.compare and decode(args.compare.read_bytes())!=result:raise ValueError('preflight repeat differs')
    with args.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps({k:v for k,v in result.items() if k!='ledger'}),flush=True)


if __name__=='__main__':main()
