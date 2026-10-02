"""Strict source-backed coding-chat review validation; never supplies judgments."""
import argparse
from datetime import datetime
import hashlib
import json
from pathlib import Path
import re
import sys
from chat_review_models import (Constraints,Document,Evidence,Importance,Judgment,MetadataField,
    NavigationTask,Packet,Review,Reviewer,Subject,TaskKind,TimeRange,Turn)


SCOPE = 'source-only-coding-chat-development-pilot-not-heldout'
CAP = 16*1024*1024


def decode(raw):
    if not isinstance(raw,bytes) or len(raw) > CAP: raise ValueError('bounded JSON bytes required')
    def unique(pairs):
        result = {}
        for key,value in pairs:
            if key in result: raise ValueError('duplicate JSON field')
            result[key] = value
        return result
    def invalid(value): raise ValueError('nonfinite JSON value')
    try: result = json.loads(raw,object_pairs_hook=unique,parse_constant=invalid)
    except (UnicodeDecodeError,json.JSONDecodeError,RecursionError): raise ValueError('valid bounded UTF-8 JSON required') from None
    pending = [(result,0)]; visited = 0
    while pending:
        item,depth = pending.pop(); visited += 1
        if depth > 64 or visited > 250000: raise ValueError('bounded review JSON structure required')
        if isinstance(item,dict): pending.extend((v,depth+1) for v in item.values())
        elif isinstance(item,list): pending.extend((v,depth+1) for v in item)
    return result


def keys(value,fields):
    if type(value) is not dict or set(value) != set(fields): raise ValueError('known schema keys required')


def text(value,empty=False):
    if not isinstance(value,str) or len(value) > 262144 or (not empty and not value.strip()): raise ValueError('bounded explicit text required')
    return value


def flag(value):
    if type(value) is not bool: raise ValueError('literal boolean declaration required')
    return value


def array(value,limit=512):
    if type(value) is not list or len(value) > limit: raise ValueError('bounded explicit array required')
    return value


def names(value):
    result = tuple(text(v) for v in array(value,64))
    if len(set(result)) != len(result): raise ValueError('unique explicit names required')
    return result


def choice(cls,value):
    if not isinstance(value,str) or value not in tuple(cls): raise ValueError('known enum value required')
    return cls(value)


def header(value):
    if type(value['schema_version']) is not int or value['schema_version'] != 1 or value['scope'] != SCOPE:
        raise ValueError('literal schema version and matching study scope required')


def packet(raw):
    value = decode(raw); keys(value,['schema_version','scope','documents']); header(value)
    documents = []; seen = set(); turn_ids = set()
    for d in array(value['documents'],32):
        keys(d,['id','turns']); identity = text(d['id'])
        if identity in seen: raise ValueError('unique document identities required')
        seen.add(identity); turns = []; previous = -1
        for t in array(d['turns']):
            keys(t,['id','ordinal','role','text','display_redacted','original_text_sha256'])
            tid = text(t['id']); ordinal = t['ordinal']; source = text(t['text'],empty=True); redacted = flag(t['display_redacted'])
            if tid in turn_ids or type(ordinal) is not int or ordinal < previous or ordinal < 0:
                raise ValueError('unique ordered original turn identities required')
            turn_ids.add(tid); previous = ordinal; pin = t['original_text_sha256']
            if not isinstance(pin,str) or not re.fullmatch('[0-9a-f]{64}',pin): raise ValueError('original source hash required')
            if not redacted and hashlib.sha256(source.encode()).hexdigest() != pin: raise ValueError('unredacted original source hash differs')
            turns.append(Turn(tid,ordinal,text(t['role']),source,redacted))
        if not turns: raise ValueError('complete nonempty indexed document required')
        documents.append(Document(identity,tuple(turns)))
    if not documents: raise ValueError('nonempty source review packet required')
    return Packet(hashlib.sha256(raw).hexdigest(),tuple(documents))


def evidence(value,document):
    turns = {t.id:t for t in document.turns}; result = []; seen = set()
    for v in array(value,64):
        keys(v,['turn_id','quote']); tid = text(v['turn_id']); quote = text(v['quote'])
        if tid not in turns or turns[tid].redacted or quote not in turns[tid].text:
            raise ValueError('exact unredacted evidence from the same document required')
        if (tid,quote) in seen: raise ValueError('duplicate source evidence')
        seen.add((tid,quote)); result.append(Evidence(tid,quote))
    if not result: raise ValueError('nonempty source evidence required')
    return tuple(result)


def timestamp(value):
    if value is None: return None
    if not isinstance(value,str) or not re.fullmatch(r'\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,6})?(?:Z|[+-]\d{2}:\d{2})',value):
        raise ValueError('explicit timezone timestamp required')
    try: parsed = datetime.fromisoformat(value.replace('Z','+00:00'))
    except ValueError: raise ValueError('valid timestamp required') from None
    if parsed.utcoffset() is None: raise ValueError('timestamp timezone required')
    return parsed


def constraints(value,unavailable,kind):
    keys(value,['provider','time_range','node'])
    provider = None if value['provider'] is None else text(value['provider'])
    node = None if value['node'] is None else text(value['node']); period = None
    if value['time_range'] is not None:
        keys(value['time_range'],['start','end']); start = timestamp(value['time_range']['start']); end = timestamp(value['time_range']['end'])
        if (start is None and end is None) or (start is not None and end is not None and start > end): raise ValueError('nonempty ordered time range required')
        period = TimeRange(start,end)
    fields = tuple(choice(MetadataField,v) for v in array(unavailable,3))
    if len(set(fields)) != len(fields): raise ValueError('unique missing metadata declarations required')
    requested = {k for k,v in [(MetadataField.PROVIDER,provider),(MetadataField.TIME,period),(MetadataField.NODE,node)] if v is not None}
    # Packet has no authenticated provider/time/node metadata. Filter values are
    # requested scopes, not source facts, and cannot become scored tasks yet.
    if set(fields) != requested: raise ValueError('every requested scope requires explicit unavailable metadata')
    if kind == TaskKind.SCOPED and not requested: raise ValueError('scoped lookup requires a requested filter')
    return Constraints(provider,period,node,frozenset(fields))


def validate(source,raw):
    value = decode(raw); keys(value,['schema_version','scope','packet_sha256','reviewer','documents']); header(value)
    if value['packet_sha256'] != source.sha256: raise ValueError('review packet fingerprint differs')
    r = value['reviewer']; keys(r,['identity','human_attestation','prior_source_exposure','candidate_output_exposure'])
    reviewer = Reviewer(text(r['identity']),flag(r['human_attestation']),flag(r['prior_source_exposure']),flag(r['candidate_output_exposure']))
    if not reviewer.human_attestation: raise ValueError('explicit human attestation required; not automatically verified')
    documents = {d.id:d for d in source.documents}; found = {}; judgments = []
    for row in array(value['documents'],32):
        keys(row,['id','sufficient_context','subjects','navigation_tasks','notes']); identity = text(row['id'])
        if identity not in documents or identity in found: raise ValueError('complete unique review document identities required')
        found[identity] = True; d = documents[identity]; sufficient = flag(row['sufficient_context']); subjects = []; tasks = []
        for s in array(row['subjects'],64):
            keys(s,['name','importance','broader_subjects','evidence','uncertainty']); name = text(s['name']); broader = names(s['broader_subjects'])
            if name in broader: raise ValueError('subject cannot be its own broader subject')
            subjects.append(Subject(name,choice(Importance,s['importance']),broader,evidence(s['evidence'],d),text(s['uncertainty'],empty=True)))
        if len({s.name for s in subjects}) != len(subjects): raise ValueError('unique subject names required')
        if subjects and not any(s.importance == Importance.PRIMARY for s in subjects): raise ValueError('at least one primary subject required when subjects exist')
        for t in array(row['navigation_tasks'],64):
            keys(t,['request','kind','evidence','constraints','unavailable_metadata']); kind = choice(TaskKind,t['kind'])
            tasks.append(NavigationTask(text(t['request']),kind,evidence(t['evidence'],d),constraints(t['constraints'],t['unavailable_metadata'],kind)))
        if len({t.request for t in tasks}) != len(tasks): raise ValueError('unique navigation requests required')
        notes = text(row['notes'],empty=True)
        if (not sufficient or not subjects or not tasks) and not notes.strip(): raise ValueError('uncertain or empty judgments require explicit notes')
        if not sufficient and (tasks or any(not s.uncertainty.strip() for s in subjects)):
            raise ValueError('insufficient context forbids navigation tasks and requires subject uncertainty')
        judgments.append(Judgment(identity,sufficient,tuple(subjects),tuple(tasks),notes))
    if set(found) != set(documents): raise ValueError('every original document must be answered')
    by_id = {d.id:d for d in judgments}
    return Review(source.sha256,reviewer,tuple(by_id[d.id] for d in source.documents))


def summary(review):
    return {'schema_version':1,'scope':'declared-human-source-backed-review-not-independent-gold',
        'packet_sha256':review.packet_sha256,'documents':len(review.documents),
        'insufficient_context':sum(not d.sufficient_context for d in review.documents),
        'subjects':sum(len(d.subjects) for d in review.documents),'tasks':sum(len(d.tasks) for d in review.documents),
        'tasks_with_unavailable_metadata':sum(bool(t.constraints.unavailable) for d in review.documents for t in d.tasks),
        'declared_no_prior_exposure':not review.reviewer.prior_source_exposure and not review.reviewer.candidate_output_exposure,
        'independence_verified':False,'semantic_quality_established':False,'agent_utility_established':False}


def read(path):
    if path.stat().st_size > CAP: raise ValueError('bounded private review file required')
    return path.read_bytes()


def main():
    p = argparse.ArgumentParser(description=__doc__); p.add_argument('--packet',type=Path,required=True); p.add_argument('--answers',type=Path,required=True); a = p.parse_args()
    try: result = summary(validate(packet(read(a.packet)),read(a.answers)))
    except (ValueError,OSError) as e:
        # Do not print transcript, submitted subjects, paths or raw JSON fragments.
        message = str(e) if isinstance(e,ValueError) else 'private review file could not be read'
        print(json.dumps({'error':'invalid_review','message':message}),file=sys.stderr); return 1
    print(json.dumps(result,allow_nan=False)); return 0


if __name__ == '__main__': sys.exit(main())
