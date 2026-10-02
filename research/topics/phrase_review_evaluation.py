"""Conditional phrase evidence evaluation; existing exposed review, no inference."""
import argparse
from dataclasses import dataclass
from enum import StrEnum
import hashlib
import json
from pathlib import Path
import resource
import time
from topic_phrases import PhraseSource
from topic_phrase_store import PhraseStore
from topic_features import terms


class Importance(StrEnum):
    PRIMARY='primary'
    SECONDARY='secondary'
    UNCERTAIN='uncertain'


@dataclass(frozen=True)
class Subject:
    thread: str
    ordinal: int
    importance: Importance
    name: str
    sources: frozenset[str]


def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()


def load(packet_path,review_path):
    if any(p.stat().st_size>8*1024*1024 for p in [packet_path,review_path]):raise ValueError('bounded private review inputs required')
    packet=json.loads(packet_path.read_text());review=json.loads(review_path.read_text())
    if packet['schema_version']!=1 or review['schema_version']!=1 or review['packet_sha256']!=digest(packet_path):raise ValueError('known source-pinned review required')
    by_thread={case['thread']:case for case in review['cases']}
    if len(by_thread)!=len(review['cases']) or set(by_thread)!={case['thread'] for case in packet['cases']}:raise ValueError('exact unique review scope required')
    sources=[];subjects=[];excluded={};uncertain=0;opaque=0
    for case in packet['cases']:
        entry=by_thread[case['thread']]
        if entry['source_sha256']!=case['source_sha256'] or len(case['turns'])!=len(entry['admission_judgments']):raise ValueError('source scope drift')
        admitted={};seen=set()
        for turn,judgment in zip(case['turns'],entry['admission_judgments']):
            kind=judgment['kind']
            if type(turn['display_redacted']) is not bool:raise ValueError('literal display-redaction flag required')
            if turn['display_redacted']:
                if kind!='sensitive':raise ValueError('redacted content cannot enter evaluation')
                opaque+=1
            elif hashlib.sha256(turn['text'].encode()).hexdigest()!=turn['text_sha256']:
                raise ValueError('unredacted original source hash mismatch')
            if (turn['id'],turn['ordinal'],turn['text_sha256'])!=(judgment['message'],judgment['ordinal'],judgment['text_sha256']) or turn['ordinal'] in seen:
                raise ValueError('original source identity/hash mismatch')
            seen.add(turn['ordinal']);kind=judgment['kind']
            if kind not in {'content','referential','protocol','attachment','sensitive'}:raise ValueError('known admission judgment required')
            if kind in {'content','referential'}:
                identity=json.dumps([case['thread'],turn['id']],ensure_ascii=False,separators=(',',':'))
                sources.append(PhraseSource(identity,case['thread'],case['provider'],turn['text']));admitted[turn['ordinal']]=identity
            else:excluded[kind]=excluded.get(kind,0)+1
        for i,subject in enumerate(entry['subjects']):
            importance=Importance(subject['importance'])
            if importance==Importance.UNCERTAIN:uncertain+=1;continue
            ordinals=subject['support_ordinals']
            if not ordinals or len(set(ordinals))!=len(ordinals) or any(ordinal not in admitted for ordinal in ordinals):raise ValueError('subject support outside admitted sources')
            subjects.append(Subject(case['thread'],i,importance,subject['subject'],frozenset(admitted[ordinal] for ordinal in ordinals)))
    if len({source.id for source in sources})!=len(sources):raise ValueError('duplicate source identity')
    return sources,subjects,{'excluded_by_review':excluded,'uncertain_subjects':uncertain,'annotator':review['annotator'],
        'review_scope':review['scope'],'oracle_admission':True,'independent_human_gold':False,'opaque_sensitive_exclusions_not_rehashed':opaque}


def support_metrics(expected,assigned):
    expected=set(expected);assigned=set(assigned);hits=len(expected&assigned)
    precision=hits/len(assigned) if assigned else 0.;recall=hits/len(expected) if expected else 0.
    return {'precision':precision,'recall':recall,'f1':2*precision*recall/(precision+recall) if precision+recall else 0.}


def evaluate(snapshot,subjects):
    topics={topic['id']:topic for topic in snapshot['topics']};threads={row['thread']:row for row in snapshot['threads']};rows=[]
    for subject in subjects:
        thread=threads[subject.thread];local=set(thread['sources']);candidates=[]
        for tag in thread['tags']:
            topic=topics[tag['topic']];candidates.append((tag['topic'],support_metrics(subject.sources,set(topic['sources'])&local)))
        candidates.sort(key=lambda pair:(-pair[1]['f1'],pair[0]))
        covered={sid for sid in subject.sources if snapshot['memberships'][sid]}
        name_atoms={word for term in terms(subject.name) for word in term.split()}
        phrase_atoms={word for tag in thread['tags'] for word in topics[tag['topic']]['phrase'].split()}
        rows.append({'thread':subject.thread,'subject_ordinal':subject.ordinal,'importance':subject.importance,
            'support_sources':len(subject.sources),'sources_with_any_selected_phrase':len(covered),
            'any_phrase_evidence_recall':len(covered)/len(subject.sources),
            'best_single_phrase':candidates[0][0] if candidates else None,
            'best_single_phrase_support':candidates[0][1] if candidates else support_metrics(subject.sources,()),
            'subject_name_atom_hit':bool(name_atoms&phrase_atoms)})
    summary={}
    for importance in [Importance.PRIMARY,Importance.SECONDARY]:
        group=[row for row in rows if row['importance']==importance]
        summary[importance]={'subjects':len(group),'mean_any_phrase_evidence_recall':sum(row['any_phrase_evidence_recall'] for row in group)/len(group),
            'mean_best_single_phrase_support_f1':sum(row['best_single_phrase_support']['f1'] for row in group)/len(group),
            'subject_name_atom_hits':sum(row['subject_name_atom_hit'] for row in group),
            'subjects_with_no_literal_evidence':sum(not row['sources_with_any_selected_phrase'] for row in group)}
    return rows,summary


def run(packet,review):
    names=['phrase_review_evaluation.py','topic_phrases.py','topic_phrase_store.py','topic_features.py','subject_labels.py','label_noise.py']
    pins={name:digest(Path(__file__).with_name(name)) for name in names};sources,subjects,policy=load(packet,review)
    start=time.perf_counter();cpu=time.process_time();store=PhraseStore('phrase_source_review_development_v1')
    for source in sources:store.upsert(source)
    snapshot=store.snapshot();rows,summary=evaluate(snapshot,subjects)
    if pins!={name:digest(Path(__file__).with_name(name)) for name in names}:raise ValueError('code drift')
    return {'schema_version':1,'scope':'conditional_existing_assistant_review_development_not_independent_quality',
        'packet_sha256':digest(packet),'review_sha256':digest(review),'code_pins':pins,'policy':policy,
        'snapshot':snapshot,'subjects':rows,'summary':summary,'semantic_quality_verified':False,'production_selected':False,
        'runtime':{'after_load_ms':(time.perf_counter()-start)*1000,'cpu_ms':(time.process_time()-cpu)*1000,
            'rusage_before_serialization_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['packet','review','output']:parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--compare',type=Path);args=parser.parse_args();result=run(args.packet,args.review)
    if args.compare:
        prior=json.loads(args.compare.read_text());current=dict(result);prior.pop('runtime');current.pop('runtime')
        if prior!=current:raise ValueError('non-runtime review results differ')
    with args.output.open('x') as file:json.dump(result,file,sort_keys=True,allow_nan=False)
    args.output.chmod(0o600)
    print(json.dumps({'diagnostics':result['snapshot']['diagnostics'],'summary':result['summary'],'runtime':result['runtime']}),flush=True)
