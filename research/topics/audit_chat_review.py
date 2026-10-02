"""Independent private chat packet source, selection and blinding audit."""
from collections import defaultdict
import argparse
import hashlib
import json
from pathlib import Path
import re
import sqlite3
import tomllib


def digest(path):
    with path.open('rb') as f: return hashlib.file_digest(f,'sha256').hexdigest()


def serial(value): return json.dumps(value,ensure_ascii=False,sort_keys=True,separators=(',',':'),allow_nan=False).encode()


def audit(database,folder):
    manifest = json.loads((folder/'provenance.json').read_bytes()); packet_bytes = (folder/'review.json').read_bytes()
    packet = json.loads(packet_bytes); originals = json.loads((folder/'unblinded-originals.json').read_bytes()); answers = json.loads((folder/'answers.json').read_bytes())
    assert type(manifest['schema_version']) is int and manifest['schema_version'] == 1
    assert manifest['seed'] == 'mash-coding-chat-source-pilot-v1-20261002' and manifest['per_stratum'] == 2
    assert manifest['policy'] == {'minimum_users':3,'maximum_users':40,'short_max_users':10,'maximum_indexed_turns':512,'maximum_original_utf8_bytes':262144}
    assert type(packet['schema_version']) is int and packet['schema_version'] == 1
    assert packet['scope'] == 'source-only-coding-chat-development-pilot-not-heldout'
    assert digest(database) == manifest['database_sha256']
    for name,pin in manifest['files_sha256'].items(): assert digest(folder/name) == pin
    for name,pin in manifest['code_pins'].items(): assert digest(Path(__file__).with_name(name)) == pin
    excluded = set()
    for path,pin in manifest['exclusions_sha256'].items():
        assert digest(Path(path)) == pin
        excluded.update(c['thread'] for c in tomllib.loads(Path(path).read_text())['cases'])
    with sqlite3.connect(database.resolve().as_uri()+'?mode=ro&immutable=1',uri=True) as db:
        user_texts = defaultdict(list)
        for thread,text in db.execute("SELECT conversation_id,content FROM messages WHERE role='user' AND length(trim(content))>0 ORDER BY conversation_id,ordinal,id"):
            user_texts[thread].append(text)
        groups = {}
        for thread,texts in user_texts.items():
            # Independent concatenation oracle, not producer incremental updater.
            framed = b'mash-chat-review-user-sequence-v1\0'+b''.join(len(t.encode()).to_bytes(8,'little')+t.encode() for t in texts)
            groups[thread] = hashlib.sha256(framed).hexdigest()
        excluded_groups = {groups[t] for t in excluded}; eligible = {}; candidate_count = 0
        for thread,provider,count,size in db.execute('SELECT c.id,c.provider,count(m.id),sum(length(CAST(m.content AS BLOB))) FROM conversations c JOIN messages m ON m.conversation_id=c.id GROUP BY c.id,c.provider ORDER BY c.id'):
            users = len(user_texts[thread])
            if provider not in ['claude-code','codex'] or not 3 <= users <= 40 or count > 512 or size > 262144: continue
            candidate_count += 1; group = groups[thread]
            if group in excluded_groups: continue
            candidate = (thread,provider,'short' if users <= 10 else 'medium')
            if group not in eligible or thread < eligible[group][0]: eligible[group] = candidate
        expected = []; support = {}
        for provider in ['claude-code','codex']:
            for stratum in ['short','medium']:
                pool = [(hashlib.sha256(serial([manifest['seed'],g])).hexdigest(),t,g)
                        for g,(t,p,s) in eligible.items() if p == provider and s == stratum]
                pool.sort(); chosen = pool[:2]; expected.extend(t for _,t,g in chosen)
                support[provider+'/'+stratum] = {'eligible_groups':len(pool),'requested':2,'selected':len(chosen)}
        assert support == manifest['strata'] and candidate_count == manifest['candidate_threads']
        assert [originals[d['id']]['thread'] for d in packet['documents']] == expected
        total = redactions = 0
        assert set(packet) == {'schema_version','scope','documents'}
        for doc in packet['documents']:
            assert set(doc) == {'id','turns'}; original = originals[doc['id']]; thread = original['thread']
            assert original['user_sequence_group'] == groups[thread] and doc['id'] == 'chat-'+groups[thread][:24]
            rows = [{'id':i,'ordinal':o,'role':r,'text':t} for i,o,r,t in db.execute('SELECT id,ordinal,role,content FROM messages WHERE conversation_id=? ORDER BY ordinal,id',(thread,))]
            assert rows == original['turns'] and hashlib.sha256(serial(rows)).hexdigest() == original['indexed_sequence_sha256']
            assert len(rows) == len(doc['turns'])
            for row,display in zip(rows,doc['turns']):
                assert set(display) == {'id','ordinal','role','text','display_redacted','original_text_sha256'}
                assert display['ordinal'] == row['ordinal'] and display['role'] == row['role']
                assert display['id'] == 'turn-'+hashlib.sha256(serial([groups[thread],row['id']])).hexdigest()[:24]
                assert display['original_text_sha256'] == hashlib.sha256(row['text'].encode()).hexdigest()
                redacted = re.fullmatch(r'[A-Za-z0-9_#=+/-]{40,}',row['text'].strip()) is not None
                assert display['display_redacted'] is redacted
                assert display['text'] == ('[redacted credential-like standalone value]' if redacted else row['text'])
                total += 1; redactions += redacted
        assert total == manifest['selected_turns'] and redactions == manifest['display_redactions']
        assert set(originals) == {d['id'] for d in packet['documents']}
    assert answers == {'schema_version':1,'scope':packet['scope'],'packet_sha256':hashlib.sha256(packet_bytes).hexdigest(),
        'reviewer':{'identity':None,'human_attestation':None,'prior_source_exposure':None,'candidate_output_exposure':None},
        'documents':[{'id':d['id'],'sufficient_context':None,'subjects':None,'navigation_tasks':None,'notes':None} for d in packet['documents']]}
    assert manifest['annotations_received'] == manifest['new_inference'] == manifest['llm_calls'] == 0
    assert digest(database) == manifest['database_sha256']
    return {'schema_version':1,'scope':'source-only-chat-pilot-audit-not-semantic-quality','auditor_sha256':digest(Path(__file__)),
        'provenance_sha256':digest(folder/'provenance.json'),'documents_checked':len(packet['documents']),'indexed_turns_checked':total,
        'source_selection_and_blank_answers':True,'limitations':['shared sqlite engine/JSON/hash primitives; no producer imports',
            'indexed snapshot and supplied exclusions only, not all native histories or prior semantic exposure',
            'no human judgments or guided-agent quality; no performance or Rust parity conclusion']}


def main():
    p = argparse.ArgumentParser(description=__doc__); p.add_argument('--database',type=Path,required=True)
    p.add_argument('--packet-directory',type=Path,required=True); p.add_argument('--output',type=Path,required=True); a = p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]): raise ValueError('fresh private audit output required')
    result = audit(a.database,a.packet_directory)
    with a.output.open('x') as f: json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps(result),flush=True)


if __name__ == '__main__': main()
