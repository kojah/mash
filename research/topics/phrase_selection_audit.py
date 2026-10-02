"""Original-source allocation and reviewed-support audit; no producer imports."""
import argparse
import hashlib
import json
import sqlite3
from pathlib import Path
from topic_phrase_audit import audit_snapshot
from phrase_review_audit import audit_data


def audit(args):
    first=json.loads(args.first.read_text());repeat=json.loads(args.repeat.read_text())
    first.pop('runtime');repeat.pop('runtime');assert first==repeat
    packet=json.loads(args.packet.read_text());review=json.loads(args.review.read_text())
    for name in ['packet','review','database','selection']:
        with getattr(args,name).open('rb') as file:
            assert hashlib.file_digest(file,'sha256').hexdigest()==first['input_hashes'][name]
    assert first['input_hashes']['packet']==review['packet_sha256']
    records=[];kinds={};cases={case['thread']:case for case in review['cases']}
    for case in packet['cases']:
        judgments=cases[case['thread']]['admission_judgments']
        assert len(judgments)==len(case['turns'])
        for turn,judgment in zip(case['turns'],judgments):
            if judgment['kind'] not in {'content','referential'}:continue
            assert not turn['display_redacted']
            identity=json.dumps([case['thread'],turn['id']],ensure_ascii=False,separators=(',',':'))
            records.append(dict(id=identity,thread=case['thread'],provider=case['provider'],text=turn['text']))
            kinds[identity]=judgment['kind']
    checks={}
    for mode,arm in first['outputs']['review'].items():
        check=audit_snapshot(records,arm['snapshot']);check['review_support']=audit_data(packet,review,arm)
        only=sum(all(kinds[sid]=='referential' for sid in topic['sources']) for topic in arm['snapshot']['topics'])
        assert only==arm['topics_with_only_reviewed_referential_evidence']
        check['referential_only_topics']=only;checks['review_'+mode]=check
    selected=json.loads(args.selection.read_text())['records'];broad=[];excluded=0
    with sqlite3.connect(args.database.resolve().as_uri()+'?mode=ro&immutable=1',uri=True) as db:
        for original,metadata in sorted(selected.items()):
            thread='thread-'+hashlib.sha256(original.encode()).hexdigest()[:24]
            provider=db.execute('SELECT provider FROM conversations WHERE id=?',(original,)).fetchone()[0]
            assert provider==metadata['provider']
            rows=db.execute("SELECT id,COALESCE(search_projection,content) FROM messages WHERE conversation_id=? AND role='user' ORDER BY ordinal,id",(original,)).fetchall()
            assert len(rows)==metadata['user_messages']
            for identity,text in rows:
                if not text:excluded+=1;continue
                encoded=json.dumps([original,identity],ensure_ascii=False,separators=(',',':')).encode()
                sid='source-'+hashlib.sha256(encoded).hexdigest()[:24]
                broad.append(dict(id=sid,thread=thread,provider=provider,text=text))
    assert len(broad)==1850 and excluded==7038
    for mode,snapshot in first['outputs']['cohort'].items():checks['cohort_'+mode]=audit_snapshot(broad,snapshot)
    return {'stable_repeat':True,'checks':checks,'semantic_quality_verified':False,
        'lifecycle_independently_replayed':False,'scope':'independent scalar counts/ranks/coverage votes/utility/ties/identities/evidence and original-review metrics',
        'shared_dependencies':'static English stoplist; original packet/review/source ledger'}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['packet','review','database','selection','first','repeat']:
        parser.add_argument('--'+name.replace('_','-'),type=Path,required=True)
    print(json.dumps(audit(parser.parse_args()),sort_keys=True))
