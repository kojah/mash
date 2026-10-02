"""Independent whole-source/token coverage audit; no inference/producer imports."""
import argparse
import hashlib
import json
import math
from pathlib import Path
from tokenizers import Tokenizer


def audit(baseline,first,repeat,tokenizer_path):
    original=json.loads(baseline.read_text());a=json.loads(first.read_text());b=json.loads(repeat.read_text())
    a.pop('runtime');b.pop('runtime');assert a==b
    assert hashlib.sha256(baseline.read_bytes()).hexdigest()==a['baseline_sha256']
    assert hashlib.sha256(tokenizer_path.read_bytes()).hexdigest()==a['tokenizer_sha256']
    tokenizer=Tokenizer.from_file(str(tokenizer_path));tokenizer.no_truncation();tokenizer.no_padding()
    records={r['id']:r for r in original['baseline_checkpoint']['records']}
    assert len(a['coverage'])==len(records)
    seen=set();window_count=admitted=excluded=source_bytes=first_bytes=0
    for row in a['coverage']:
        sid=row['source'];assert sid not in seen;seen.add(sid)
        text=records[sid]['text'];parts=row['windows'];position=byte_position=0;payload=[]
        assert hashlib.sha256(text.encode()).hexdigest()==row['source_text_sha256']
        assert row['source_characters']==len(text) and row['source_bytes']==len(text.encode())
        for ordinal,part in enumerate(parts):
            assert part['source']==sid and part['ordinal']==ordinal
            assert part['start']==position and part['byte_start']==byte_position
            segment=text[part['start']:part['end']]
            assert segment and hashlib.sha256(segment.encode()).hexdigest()==part['text_sha256']
            assert part['byte_end']-part['byte_start']==len(segment.encode())
            encoding=tokenizer.encode(segment);bare=tokenizer.encode(segment,add_special_tokens=False)
            assert list(encoding.ids)==part['ids'] and list(encoding.type_ids)==part['types']
            assert 2<=len(encoding.ids)<=256 and len(bare.ids)==part['payload_tokens']
            payload.extend(bare.ids);position=part['end'];byte_position=part['byte_end'];window_count+=1
        if parts:
            assert row['status']=='admitted' and records[sid]['vector'] is not None
            assert position==len(text) and byte_position==len(text.encode())
            assert payload==tokenizer.encode(text,add_special_tokens=False).ids
            assert row['first_window_bytes']==parts[0]['byte_end'];admitted+=1
        else:
            assert row['status']=='excluded_empty_projection' and text=='' and records[sid]['vector'] is None
            excluded+=1
        assert byte_position==row['complete_bytes']
        source_bytes+=byte_position;first_bytes+=row['first_window_bytes']
        for arm in a['arms'].values():
            vector=arm['vectors'][sid]
            if parts:
                assert len(vector)==384 and all(math.isfinite(v) for v in vector)
                assert abs(math.sqrt(math.fsum(v*v for v in vector))-1)<1e-12
            else:assert vector is None
        if len(parts)==1:
            assert a['arms']['first_window']['vectors'][sid]==a['arms']['complete_windows']['vectors'][sid]
    assert seen==set(records) and window_count==a['inference_calls']
    for arm in a['arms'].values():
        for key in ['raw_forward','raw_reverse','unit_forward','unit_reverse']:
            snapshot=arm[key];assert set(snapshot['versions'])==set(records)==set(snapshot['memberships'])
            assert all(not snapshot['memberships'][sid] for sid,r in records.items() if r['vector'] is None)
    return {'stable_repeat':True,'sources':len(records),'admitted':admitted,'excluded':excluded,
            'windows':window_count,'complete_bytes':source_bytes,'first_window_bytes':first_bytes,
            'scope':'original source coverage, contiguous bytes/characters, whole-source token equality, vector normalization and exclusion conservation',
            'shared_kernel':'tokenizers compiled model; no producer/window helper imports',
            'inference_routing_labels_semantic_quality_verified':False}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['baseline','first','repeat','tokenizer']:parser.add_argument('--'+name,type=Path,required=True)
    args=parser.parse_args();print(json.dumps(audit(args.baseline,args.first,args.repeat,args.tokenizer),sort_keys=True))
