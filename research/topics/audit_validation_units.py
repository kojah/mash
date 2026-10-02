"""Independent native-text/token/readiness and greedy source-boundary audit."""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import re
from tokenizers import Tokenizer


def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()


def audit(root, assets, path):
    data = json.loads(path.read_text()); source = root/'topic-qmsum-author-validation-20261002/data/ALL/jsonl/val.jsonl'
    assert digest(source) == data['source_sha256'] == 'fc7444e241bdbe6997c01720d07f1a58b7357284acb73f909766e5a4700db49f'
    assert digest(assets/'tokenizer.json') == data['tokenizer_sha256'] == 'da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0'
    for name,pin in data['code_pins'].items():assert digest(Path(__file__).with_name(name)) == pin
    tokenizer = Tokenizer.from_file(str(assets/'tokenizer.json'));tokenizer.no_padding();tokenizer.no_truncation()
    statuses = {'raw':Counter(),'masked':Counter()};seen = ready_total = block_total = 0
    native = [json.loads(line) for line in source.read_bytes().splitlines()]
    for m,meeting in enumerate(native):
        expected = data['meetings'][m];assert expected['meeting'] == m and expected['source_offset'] == seen
        records = [];flags = [];tokens = []
        for i,turn in enumerate(meeting['meeting_transcripts']):
            original = turn['content'];masked = re.sub(r'\{(?:disfmarker|vocalsound)\}',lambda hit:' '*len(hit.group()),original);variants = {}
            for name,text in [('raw',original),('masked',masked)]:
                n = len(tokenizer.encode(text).ids);state = 'empty_or_marker_only' if not text.strip() else 'over_model_window' if n > 512 else 'ready'
                variants[name] = {'text_sha256':hashlib.sha256(text.encode()).hexdigest(),'tokens':n,'status':state};statuses[name][state] += 1
            flag = all(v['status'] == 'ready' for v in variants.values());flags.append(flag);tokens.append(variants['raw']['tokens'])
            record = {'meeting':m,'ordinal':i,'source_row':seen+i,'ready':flag,'variants':variants}
            assert record == data['rows'][seen+i];records.append(record)
        blocks = [];owners = [-1]*len(records);i = 0
        while i < len(flags):
            if not flags[i]:i += 1;continue
            start = i;total = 0
            while i < len(flags) and flags[i] and i-start < 16 and total+tokens[i] <= 512:
                total += tokens[i];i += 1
            reason = 'meeting_end' if i == len(flags) else 'unsupported_source_gap' if not flags[i] else 'sixteen_turn_limit' if i-start == 16 else 'original_model_token_limit'
            ordinal = len(blocks);blocks.append({'ordinal':ordinal,'start':start,'end':i-1,'tokens':total,'close':reason});owners[start:i] = [ordinal]*(i-start)
        assert expected == {'meeting':m,'source_offset':seen,'source_count':len(records),'blocks':blocks,'owners':owners}
        seen += len(records);ready_total += sum(flags);block_total += len(blocks)
    assert len(native) == 35 and seen == len(data['rows']) == 20123
    assert data['support'] == {'sources':seen,'ready':ready_total,'blocks':block_total,'status_counts':{k:dict(v) for k,v in statuses.items()}}
    return {'capture_sha256':digest(path),'auditor_sha256':digest(Path(__file__)),'sources_checked':seen,'blocks_checked':block_total,
        'limitations':['independent regex/readiness/greedy boundaries;shared pinned tokenizer engine/assets',
            'source-only support audit,not lexical features/topology/query quality or inference/Rust/ingestion parity']}


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    for name in ['artifacts','assets','capture','output']:p.add_argument('--'+name,type=Path,required=True)
    a = p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    result = audit(a.artifacts,a.assets,a.capture)
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps(result),flush=True)
