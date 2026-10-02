"""Deterministic private coding-chat pilot; no model outputs or judgments."""
import argparse
from collections import Counter
from dataclasses import dataclass
import hashlib
import html
import json
from pathlib import Path
import sqlite3
import time
import tomllib
from review_packets import review_text


SEED = 'mash-coding-chat-source-pilot-v1-20261002'
STRATA = [(p,s) for p in ['claude-code','codex'] for s in ['short','medium']]


def digest(path):
    result = hashlib.sha256()
    with path.open('rb') as source:
        for block in iter(lambda:source.read(1024*1024),b''): result.update(block)
    return result.hexdigest()


def encoded(value): return json.dumps(value,ensure_ascii=False,sort_keys=True,separators=(',',':'),allow_nan=False).encode()


def group(texts):
    """Ordered nonempty original user sequence; length framing prevents collisions."""
    result = hashlib.sha256(b'mash-chat-review-user-sequence-v1\0')
    for text in texts:
        raw = text.encode(); result.update(len(raw).to_bytes(8,'little')); result.update(raw)
    return result.hexdigest()


@dataclass(frozen=True)
class Candidate:
    thread: str
    provider: str
    stratum: str
    group: str


def select(candidates, excluded, per_stratum=2):
    if type(per_stratum) is not int or not 1 <= per_stratum <= 8: raise ValueError('bounded literal sample count required')
    aliases = {}; seen = set()
    for c in candidates:
        if (not isinstance(c.thread,str) or not c.thread or c.thread in seen or (c.provider,c.stratum) not in STRATA
                or not isinstance(c.group,str) or len(c.group) != 64 or any(v not in '0123456789abcdef' for v in c.group)):
            raise ValueError('unique valid candidate identities and canonical hashes required')
        seen.add(c.thread)
        if c.group in excluded: continue
        # Deterministic representative, independent of input/SQL traversal order.
        if c.group not in aliases or c.thread < aliases[c.group].thread: aliases[c.group] = c
    chosen = []; support = {}
    for p,s in STRATA:
        bucket = [c for c in aliases.values() if (c.provider,c.stratum) == (p,s)]
        bucket.sort(key=lambda c:(hashlib.sha256(encoded([SEED,c.group])).hexdigest(),c.thread))
        support[p+'/'+s] = {'eligible_groups':len(bucket),'requested':per_stratum,'selected':min(per_stratum,len(bucket))}
        chosen.extend(bucket[:per_stratum])
    return chosen,support


def page(packet,rubric):
    cards = []
    for document in packet['documents']:
        turns = ''.join('<section><h3>Turn '+str(t['ordinal'])+' · '+html.escape(t['role'])+'</h3><pre>'
                        +html.escape(t['text'])+'</pre></section>' for t in document['turns'])
        cards.append('<article><h2>'+html.escape(document['id'])+'</h2>'+turns+'</article>')
    return ('<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width">'
        '<meta http-equiv="Content-Security-Policy" content="default-src &#39;none&#39;; style-src &#39;unsafe-inline&#39;">'
        '<title>Coding chat source review</title><style>body{font:16px/1.5 system-ui;max-width:1000px;margin:2rem auto;padding:1rem}'
        'pre{white-space:pre-wrap;overflow-wrap:anywhere}article{border-top:1px solid #aaa;margin-top:3rem}</style>'
        '<h1>Coding chat source review</h1><pre>'+html.escape(rubric)+'</pre>'+''.join(cards)+'</html>')


def answers(packet,pin):
    return {'schema_version':1,'scope':packet['scope'],'packet_sha256':pin,
        'reviewer':{'identity':None,'human_attestation':None,'prior_source_exposure':None,'candidate_output_exposure':None},
        'documents':[{'id':d['id'],'sufficient_context':None,'subjects':None,'navigation_tasks':None,'notes':None} for d in packet['documents']]}


def build(database, exclusions, rubric):
    begin = time.perf_counter(); pin = digest(database)
    if Path(str(database)+'-wal').exists() and Path(str(database)+'-wal').stat().st_size: raise ValueError('frozen snapshot without live WAL required')
    excluded_ids = set(); exclusion_pins = {}
    for path in exclusions:
        raw = path.read_bytes(); spec = tomllib.loads(raw.decode())
        if type(spec.get('schema_version')) is not int or spec['schema_version'] != 1 or not isinstance(spec.get('cases'),list): raise ValueError('original annotation schema required')
        for case in spec['cases']:
            if not isinstance(case.get('thread'),str) or not case['thread']: raise ValueError('excluded source identity required')
            excluded_ids.add(case['thread'])
        exclusion_pins[str(path)] = hashlib.sha256(raw).hexdigest()
    all_users = {}; candidates = []; original = {}; providers = Counter()
    with sqlite3.connect(database.resolve().as_uri()+'?mode=ro&immutable=1',uri=True) as connection:
        for thread,text in connection.execute("SELECT conversation_id,content FROM messages WHERE role='user' AND length(trim(content))>0 ORDER BY conversation_id,ordinal,id"):
            if not isinstance(text,str): raise ValueError('original source text required')
            all_users.setdefault(thread,[]).append(text)
        groups = {t:group(texts) for t,texts in all_users.items()}
        if not excluded_ids <= groups.keys(): raise ValueError('excluded histories missing from frozen snapshot')
        excluded_groups = {groups[t] for t in excluded_ids}
        metadata = connection.execute('SELECT c.id,c.provider,count(m.id),sum(length(CAST(m.content AS BLOB))) FROM conversations c JOIN messages m ON m.conversation_id=c.id GROUP BY c.id,c.provider ORDER BY c.id')
        for thread,provider,turn_count,byte_count in metadata:
            providers[provider] += 1; user_count = len(all_users.get(thread,[]))
            if provider not in ['claude-code','codex'] or not 3 <= user_count <= 40 or turn_count > 512 or byte_count > 262144: continue
            candidates.append(Candidate(thread,provider,'short' if user_count <= 10 else 'medium',groups[thread]))
        selected,support = select(candidates,excluded_groups)
        documents = []
        for candidate in selected:
            rows = list(connection.execute('SELECT id,ordinal,role,content FROM messages WHERE conversation_id=? ORDER BY ordinal,id',(candidate.thread,)))
            if len({r[0] for r in rows}) != len(rows) or any(type(r[1]) is not int or not isinstance(r[2],str) or not isinstance(r[3],str) for r in rows):
                raise ValueError('complete unique original indexed turns required')
            opaque = 'chat-'+candidate.group[:24]; originals = []; turns = []
            for identity,ordinal,role,text in rows:
                displayed,redacted = review_text(text)
                originals.append({'id':identity,'ordinal':ordinal,'role':role,'text':text})
                turns.append({'id':'turn-'+hashlib.sha256(encoded([candidate.group,identity])).hexdigest()[:24],
                    'ordinal':ordinal,'role':role,'text':displayed,'display_redacted':redacted,'original_text_sha256':hashlib.sha256(text.encode()).hexdigest()})
            documents.append({'id':opaque,'turns':turns})
            original[opaque] = {'thread':candidate.thread,'provider':candidate.provider,'stratum':candidate.stratum,
                'user_sequence_group':candidate.group,'indexed_sequence_sha256':hashlib.sha256(encoded(originals)).hexdigest(),'turns':originals}
    packet = {'schema_version':1,'scope':'source-only-coding-chat-development-pilot-not-heldout','documents':documents}
    packet_bytes = encoded(packet); packet_pin = hashlib.sha256(packet_bytes).hexdigest()
    files = {'review.json':packet_bytes,'review.html':page(packet,rubric).encode(),'answers.json':encoded(answers(packet,packet_pin)),
             'unblinded-originals.json':encoded(original)}
    code_pins = {n:digest(Path(__file__).with_name(n)) for n in ['chat_review_packet.py','review_packets.py','chat-review-rubric.md']}
    if pin != digest(database) or any(digest(Path(p)) != h for p,h in exclusion_pins.items()): raise ValueError('source drift during capture')
    manifest = {'schema_version':1,'scope':'private-coding-chat-packet-preparation-no-human-judgments',
        'database_sha256':pin,'exclusions_sha256':exclusion_pins,'code_pins':code_pins,'seed':SEED,'per_stratum':2,
        'policy':{'minimum_users':3,'maximum_users':40,'short_max_users':10,'maximum_indexed_turns':512,'maximum_original_utf8_bytes':262144},
        'strata':support,'excluded_threads':len(excluded_ids),'excluded_user_sequence_groups':len(excluded_groups),
        'snapshot_providers':dict(providers),'candidate_threads':len(candidates),'selected_documents':len(documents),
        'selected_turns':sum(len(d['turns']) for d in documents),'display_redactions':sum(t['display_redacted'] for d in documents for t in d['turns']),
        'annotations_received':0,'llm_calls':0,'new_inference':0,'files_sha256':{n:hashlib.sha256(v).hexdigest() for n,v in files.items()},
        'limitations':['already exposed original index snapshot; not genuinely new or held-out native histories',
            'complete indexed sequence is not proof of upstream/native history completeness; embedded text is quoted data',
            'exclusion covers supplied review sidecars and exact ordered user-sequence aliases, not semantic near duplicates or all historical exposure',
            'display guard only redacts standalone credential-like values; not a general secret scanner; private originals retained separately',
            'blank answers are unanswered; no independent human labels, topic quality, agent utility or performance conclusion'],
        'timings':{'preparation_ms':(time.perf_counter()-begin)*1000}}
    files['provenance.json'] = encoded(manifest)
    return files,manifest


def main():
    p = argparse.ArgumentParser(description=__doc__); p.add_argument('--database',type=Path,required=True)
    p.add_argument('--exclude-annotations',type=Path,action='append',required=True); p.add_argument('--output-directory',type=Path,required=True)
    p.add_argument('--compare',type=Path); a = p.parse_args()
    if a.output_directory.exists() or a.output_directory.resolve().is_relative_to(Path(__file__).resolve().parents[2]): raise ValueError('fresh private outside-Git directory required')
    files,manifest = build(a.database,a.exclude_annotations,Path(__file__).with_name('chat-review-rubric.md').read_text())
    if a.compare:
        for name,value in files.items():
            old = (a.compare/name).read_bytes()
            if name == 'provenance.json':
                left = json.loads(old); right = json.loads(value); left.pop('timings'); right.pop('timings')
                if left != right: raise ValueError('provenance repeat differs')
            elif old != value: raise ValueError('complete private packet repeat differs')
    a.output_directory.mkdir(mode=0o700)
    for name,value in files.items():
        with (a.output_directory/name).open('xb') as target: target.write(value)
        (a.output_directory/name).chmod(0o600)
    print(json.dumps({k:manifest[k] for k in ['selected_documents','selected_turns','strata','display_redactions','annotations_received','timings']}),flush=True)


if __name__ == '__main__': main()
