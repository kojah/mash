"""Count human-declared topic judgments; never establishes gold or promotion."""
import argparse
from dataclasses import dataclass
from enum import StrEnum
from fractions import Fraction
import hashlib
import math
from pathlib import Path
import random
import re
import json
import sys
from chat_review import array,choice,decode,flag,keys,names,read,text
from chat_review_models import Importance


class Exposure(StrEnum):
    UNREVIEWED='unreviewed'
    DEVELOPMENT='exposed-development'
    HELDOUT='declared-heldout-human'


class Provider(StrEnum):
    CLAUDE='claude-code'
    CODEX='codex'
    AIDER='aider'
    HERMES='hermes'
    OPENCODE='opencode'
    OPENCLAW='openclaw'
    PI='pi'
    COPILOT='copilot'
    GEMINI='gemini-cli'
    CLINE='cline'
    CURSOR='cursor'
    WINDSURF='windsurf'
    ANTIGRAVITY='antigravity'


@dataclass(frozen=True)
class Tag:
    id: str
    useful: bool
    sources: frozenset[str]


@dataclass(frozen=True)
class Subject:
    id: str
    importance: Importance
    uncertain: bool
    sources: frozenset[str]
    tag: str | None


@dataclass(frozen=True)
class Thread:
    id: str
    provider: Provider
    tags: tuple[Tag,...]
    subjects: tuple[Subject,...]


@dataclass(frozen=True)
class Review:
    sha256: str
    packet_sha256: str
    candidate_sha256: str
    exposure: Exposure
    human: bool
    prior_source_exposure: bool
    outputs_seen_before_subjects: bool
    threads: tuple[Thread,...]


def pin(value):
    if not isinstance(value,str) or not re.fullmatch('[0-9a-f]{64}',value):raise ValueError('explicit SHA256 pin required')
    return value


def source_names(value):
    result=tuple(text(item) for item in array(value,2048))
    if len(set(result))!=len(result):raise ValueError('unique original source identities required')
    return result


def parse(raw):
    value=decode(raw);keys(value,['schema_version','scope','packet_sha256','candidate_sha256','exposure','reviewer','threads'])
    if type(value['schema_version']) is not int or value['schema_version']!=1 or value['scope']!='topic-quality-declared-counts-v1':
        raise ValueError('known declared-quality scope required')
    reviewer=value['reviewer'];keys(reviewer,['identity','human_attestation','prior_source_exposure','outputs_seen_before_subjects'])
    text(reviewer['identity']);threads=[];seen=set()
    for row in array(value['threads'],128):
        keys(row,['id','provider','source_ids','emitted_tag_ids','tags','subjects']);identity=text(row['id'])
        if identity in seen:raise ValueError('unique thread identities required')
        seen.add(identity);provider=choice(Provider,row['provider']);sources=frozenset(source_names(row['source_ids']));emitted=set(names(row['emitted_tag_ids']))
        if len(emitted)>8:raise ValueError('maximum eight emitted thread tags required')
        tags={}
        for tag in array(row['tags'],8):
            keys(tag,['id','accurate','specific','source_supported','evidence_source_ids']);tid=text(tag['id'])
            if tid in tags or tid not in emitted:raise ValueError('unique emitted-tag judgments required')
            evidence=frozenset(names(tag['evidence_source_ids']))
            if not evidence<=sources:raise ValueError('tag evidence outside thread')
            accurate=flag(tag['accurate']);specific=flag(tag['specific']);supported=flag(tag['source_supported'])
            if supported and not evidence:raise ValueError('source-supported tag requires evidence')
            tags[tid]=Tag(tid,accurate and specific and supported,evidence)
        if set(tags)!=emitted:raise ValueError('every emitted tag requires a judgment')
        subjects=[];used_tags=set();seen_subjects=set()
        for subject in array(row['subjects'],64):
            keys(subject,['id','importance','uncertain','evidence_source_ids','matched_tag_id']);sid=text(subject['id'])
            if sid in seen_subjects:raise ValueError('unique reviewed subject identities required')
            seen_subjects.add(sid);importance=choice(Importance,subject['importance']);uncertain=flag(subject['uncertain'])
            evidence=frozenset(names(subject['evidence_source_ids']))
            if not evidence or not evidence<=sources:raise ValueError('nonempty original subject evidence required')
            matched=subject['matched_tag_id']
            if matched is not None:
                matched=text(matched)
                if uncertain or matched not in tags or matched in used_tags:raise ValueError('distinct known matches for certain subjects required')
                if not tags[matched].useful or not evidence&tags[matched].sources:raise ValueError('match requires useful tag and shared original evidence')
                used_tags.add(matched)
            subjects.append(Subject(sid,importance,uncertain,evidence,matched))
        threads.append(Thread(identity,provider,tuple(tags[key] for key in sorted(tags)),tuple(sorted(subjects,key=lambda s:s.id))))
    return Review(hashlib.sha256(raw).hexdigest(),pin(value['packet_sha256']),pin(value['candidate_sha256']),choice(Exposure,value['exposure']),
        flag(reviewer['human_attestation']),flag(reviewer['prior_source_exposure']),flag(reviewer['outputs_seen_before_subjects']),
        tuple(sorted(threads,key=lambda thread:thread.id)))


def totals(threads):
    useful=sum(tag.useful for thread in threads for tag in thread.tags);tags=sum(len(thread.tags) for thread in threads)
    certain=[subject for thread in threads for subject in thread.subjects if not subject.uncertain]
    primary=[subject for subject in certain if subject.importance==Importance.PRIMARY]
    secondary=[subject for subject in certain if subject.importance==Importance.SECONDARY]
    return ((useful,tags),(sum(s.tag is not None for s in primary),len(primary)),
        (sum(s.tag is not None for s in secondary),len(secondary)))


def intervals(threads,draws=5000):
    rng=random.Random(718);values=[[],[],[]]
    for _ in range(draws if threads else 0):
        sampled=[threads[rng.randrange(len(threads))] for _ in threads]
        for group,(numerator,denominator) in zip(values,totals(sampled)):
            if denominator:group.append(numerator/denominator)
    result=[]
    for group in values:
        group.sort();n=len(group)
        result.append({'lower':group[math.floor((n-1)*.025)] if n else None,
            'upper':group[math.ceil((n-1)*.975)] if n else None,'valid_resamples':n,'total_resamples':draws if threads else 0})
    return result


def score(review):
    counts=totals(review.threads);thresholds=(Fraction(17,20),Fraction(4,5),Fraction(3,5));names_=('tag_precision','primary_coverage','secondary_coverage')
    metrics={}
    for name,(numerator,denominator),threshold,interval in zip(names_,counts,thresholds,intervals(review.threads)):
        metrics[name]={'numerator':numerator,'denominator':denominator,'value':numerator/denominator if denominator else None,
            'target':float(threshold),'target_met':bool(denominator and Fraction(numerator,denominator)>=threshold),'thread_bootstrap_95':interval}
    eligible=[thread for thread in review.threads if any(not s.uncertain for s in thread.subjects)]
    certain=sum(not subject.uncertain for thread in review.threads for subject in thread.subjects)
    scope={'threads':len(review.threads),'threads_with_certain_subjects':len(eligible),
        'providers_with_certain_subjects':len({thread.provider for thread in eligible}),
        'certain_subjects':certain,'uncertain_subjects':sum(s.uncertain for thread in review.threads for s in thread.subjects)}
    scope_met=len(eligible)>=20 and scope['providers_with_certain_subjects']>=2 and certain>=60 and counts[1][1]>=20 and counts[2][1]>=20
    declared_eligible=review.exposure==Exposure.HELDOUT and review.human and not review.prior_source_exposure and not review.outputs_seen_before_subjects
    return {'schema_version':1,'scope':'declared_topic_quality_counts_not_verified_gold',
        'review_sha256':review.sha256,'packet_sha256':review.packet_sha256,'candidate_sha256':review.candidate_sha256,'evaluation_scope':scope,
        'metrics':metrics,'numeric_targets_met':all(row['target_met'] for row in metrics.values()),
        'minimum_scope_met':scope_met,'declared_review_eligible':declared_eligible,
        'declared_quality_gate_met':scope_met and declared_eligible and all(row['target_met'] for row in metrics.values()),
        'independence_verified':False,'source_provenance_verified':False,'semantic_quality_verified':False,
        'production_selected':False,'remaining_gates':['independent review/split/source provenance audit',
            'persisted lifecycle and ownership','complete ingestion budget','search safety','selected executable reference and Rust parity'],
        'bootstrap':'5000 seeded718 thread resamples; empirical floor/ceil 2.5/97.5 percentiles; undefined ratios reported, not replaced by zero'}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--review',type=Path,required=True);args=parser.parse_args()
    try:result=score(parse(read(args.review)))
    except (ValueError,OSError) as error:
        message=str(error) if isinstance(error,ValueError) else 'private review file could not be read'
        print(json.dumps({'error':'invalid_topic_quality_review','message':message}),file=sys.stderr)
        sys.exit(1)
    print(json.dumps(result,sort_keys=True,allow_nan=False))
