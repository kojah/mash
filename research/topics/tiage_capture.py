"""Capture only pinned TIAGE author training data and provenance; no models."""
import argparse
import hashlib
import json
from pathlib import Path
import urllib.request


COMMIT='08f98dfa066a5f2fba2bbd1d6ed6ee9a783a2b1d'
REPOSITORY='https://github.com/HuiyuanXie/tiage'
FILES={
    'README.md':('aff5230600952ee0a3baa8b1b74b5158371f3e93',2396),
    'LICENSE':('3b544bebaddeb5fe064c70114b11c102abbef40c',1068),
    'data/personachat/anno/train/anno_train.json':('96a5d7dfca7e0c7cd53205b14b41fb423068a674',295186),
    'scripts/ts_cls/bert/train_bert_classifier.py':('aef6c08c3d74c8ff0d6690ab22ee11a6ee955114',6602),
    'scripts/ts_cls/bert/ts_classifier_bert.py':('7f159fac5d8bc7a491b6ac35d01f1f14f1b86de2',749),
}


def verify(raw,blob,size):
    # Git SHA1 identifies the author blob; SHA256 independently pins our capture.
    actual=hashlib.sha1(b'blob '+str(len(raw)).encode()+b'\0'+raw).hexdigest()
    if len(raw)!=size or actual!=blob:raise ValueError('author blob/size drift')
    return hashlib.sha256(raw).hexdigest()


def capture(destination):
    if destination.exists() or destination.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git capture required')
    destination.mkdir(parents=True);records={}
    for path,(blob,size) in FILES.items():
        url=f'https://raw.githubusercontent.com/HuiyuanXie/tiage/{COMMIT}/{path}'
        with urllib.request.urlopen(url,timeout=30) as response:raw=response.read(size+1)
        pin=verify(raw,blob,size);target=destination/path;target.parent.mkdir(parents=True,exist_ok=True)
        with target.open('xb') as f:f.write(raw)
        records[path]={'git_blob':blob,'sha256':pin,'bytes':size,'url':url}
        print(json.dumps({'captured':path,'sha256':pin}),flush=True)
    result={'schema_version':1,'repository':REPOSITORY,'commit':COMMIT,'files':records,
            'scope':'author-annotated-training-only-structural-preflight',
            'development_test_weak_supervision_downloaded':False,'inference_calls':0}
    with (destination/'capture.json').open('x') as f:json.dump(result,f,indent=2)


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output-directory',type=Path,required=True)
    capture(p.parse_args().output_directory)


if __name__=='__main__':main()
