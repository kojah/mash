"""Independent source-support and structural audit of capacity controls."""
from collections import Counter
import argparse
import json
import math
from pathlib import Path
import re
from sklearn.feature_extraction.text import ENGLISH_STOP_WORDS


STOP=set(ENGLISH_STOP_WORDS)|{'yeah','okay','ok','uh','um','erm','hmm','mm','oh'}
TOKEN=re.compile(r'(?u)\b\w[\w.+:/-]*\b')


def counts(text):
    output=Counter();prior=None;end=0
    for match in TOKEN.finditer(text):
        word=match.group().lower()
        if word in STOP or len(word)>64:prior=None
        else:
            output[word]+=1
            if prior is not None and text[end:match.start()].isspace():output[prior+' '+word]+=1
            prior=word
        end=match.end()
    return output


def audit(baseline,first,repeat):
    original=json.loads(baseline.read_text());a=json.loads(first.read_text());b=json.loads(repeat.read_text())
    a.pop('runtime');b.pop('runtime');assert a==b
    source={r['id']:r for r in original['baseline_checkpoint']['records']}
    words={sid:counts(r['text']) for sid,r in source.items()}
    frequency=Counter(term for row in words.values() for term in row)
    checked_nodes=checked_labels=0
    for name,arm in a['arms'].items():
        for direction in ['forward','reverse']:
            snapshot=arm[direction];nodes={n['id']:n for n in snapshot['nodes']}
            assert set(snapshot['versions'])==set(source)==set(snapshot['memberships'])
            assert len(nodes)==len(snapshot['nodes'])
            seen=set()
            def walk(identity,parent):
                nonlocal checked_nodes,checked_labels
                assert identity not in seen;seen.add(identity);node=nodes[identity]
                assert node['parent']==parent
                if node['children']:
                    assert len(node['children'])==2
                    members=set.union(*(walk(child,identity) for child in node['children']))
                else:
                    members={sid for sid,tags in snapshot['memberships'].items() if any(tag['topic']==identity for tag in tags)}
                assert set(node['sources'])==members and len(node['sources'])==len(members)
                for label in node['labels']:
                    term=label['term'];support=sum(term in words[sid] for sid in members)
                    assert support==label['source_support'] and support>=2
                    assert frequency[term]<=.4*len(source) and any(c.isalpha() for c in term)
                    assert math.isfinite(label['score']) and label['score']>0
                    checked_labels+=1
                assert node['label_support']==('supported' if len(node['labels'])>=2 else 'weak')
                checked_nodes+=1;return members
            walk(snapshot['root'],None);assert seen==set(nodes)
            for sid,tags in snapshot['memberships'].items():
                assert len(tags)<=3 and len({tag['topic'] for tag in tags})==len(tags)
                if tags:
                    best=max(t['score'] for t in tags)
                    assert all(math.isfinite(t['score']) and t['score']>=.55 and t['score']>=best*.8 and t['score']<=1+1e-12 for t in tags)
            for row in snapshot['threads']:
                ids={sid for sid,r in source.items() if r['thread']==row['thread']}
                direct={tag['topic'] for sid in ids for tag in snapshot['memberships'][sid]}
                ancestors=set()
                for identity in direct:
                    parent=nodes[identity]['parent']
                    while parent is not None:ancestors.add(parent);parent=nodes[parent]['parent']
                assert set(row['direct_tags'])==direct and set(row['ancestor_tags'])==ancestors
            if name=='cap128_every16':assert snapshot==original[direction]
    for direction in ['forward','reverse']:
        assert a['arms']['cap2000_every16'][direction]==a['arms']['cap2000_final_only'][direction]
    return {'stable_repeat':True,'baseline_semantics_exact':True,'schedule_final_semantics_exact':True,
            'snapshots':6,'nodes_checked':checked_nodes,'labels_support_checked':checked_labels,
            'sources_per_snapshot':len(source),'shared_static_vocabulary':'sklearn English stoplist',
            'scope':'source-support, DF, memberships bounds, tree/thread unions, repeat/default/schedule equality',
            'routing_label_scores_semantic_quality_verified':False}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['baseline','first','repeat']:parser.add_argument('--'+name,type=Path,required=True)
    args=parser.parse_args();print(json.dumps(audit(args.baseline,args.first,args.repeat),sort_keys=True))
