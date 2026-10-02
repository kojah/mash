"""Typed complete QMSum training source; topic/query annotations never become features."""
import argparse
from collections import Counter
from dataclasses import dataclass
from enum import StrEnum
import hashlib
import json
from pathlib import Path
import platform
import re
import resource
import time

from qmsum_capture import COMMIT, FILES, REPOSITORY, TREE
from stage_parity import decode
from tiage_capture import verify


@dataclass(frozen=True)
class Span:
    start: int
    end: int
    original_start: str
    original_end: str


@dataclass(frozen=True)
class Turn:
    ordinal: int
    speaker: str
    text: str


@dataclass(frozen=True)
class Topic:
    ordinal: int
    name: str
    spans: tuple[Span, ...]


@dataclass(frozen=True)
class GeneralQuery:
    ordinal: int
    query: str
    answer: str


@dataclass(frozen=True)
class SpecificQuery:
    ordinal: int
    query: str
    answer: str
    spans: tuple[Span, ...]


@dataclass(frozen=True)
class Meeting:
    ordinal: int
    raw_line_sha256: str
    turns: tuple[Turn, ...]
    topics: tuple[Topic, ...]
    general_queries: tuple[GeneralQuery, ...]
    specific_queries: tuple[SpecificQuery, ...]


class Transition(StrEnum):
    STABLE = 'annotated_membership_stable'
    SINGLETON_SWITCH = 'annotated_singleton_topic_switch'
    OVERLAP_CHANGE = 'annotated_multi_membership_change'
    GAP = 'annotation_gap_transition'
    UNKNOWN = 'both_turns_unannotated'


def text(value, nonempty=False):
    if type(value) is not str or len(value.encode()) > 1048576 or (nonempty and not value.strip()):
        raise ValueError('bounded original string required')
    return value


def fields(value, expected):
    if type(value) is not dict or set(value) != set(expected):
        raise ValueError('exact original QMSum object fields required')


def sequence(value, nonempty=False):
    if type(value) is not list or len(value) > 10000 or (nonempty and not value):
        raise ValueError('bounded original list required')
    return value


def spans(value, turns):
    result = []
    for row in sequence(value, nonempty=True):
        if (type(row) is not list or len(row) != 2 or any(type(n) is not str or re.fullmatch(r'[0-9]{1,9}', n) is None for n in row)):
            raise ValueError('original string-index span pair required')
        start, end = map(int, row)
        if not 0 <= start <= end < turns:
            raise ValueError('inclusive topic/query span outside original transcript')
        result.append(Span(start, end, *row))
    return tuple(result)


def parse(raw):
    if type(raw) is not bytes or not raw or len(raw) > 134217728:
        raise ValueError('bounded original JSONL bytes required')
    lines = raw.splitlines()
    if not 0 < len(lines) <= 1000:raise ValueError('bounded complete original meetings required')
    result = []
    for ordinal, line in enumerate(lines):
        data = decode(line)
        fields(data, ['topic_list', 'general_query_list', 'specific_query_list', 'meeting_transcripts'])
        turns = []
        for i, row in enumerate(sequence(data['meeting_transcripts'], nonempty=True)):
            fields(row, ['speaker', 'content']);turns.append(Turn(i, text(row['speaker']), text(row['content'])))
        topics = []
        for i, row in enumerate(sequence(data['topic_list'], nonempty=True)):
            fields(row, ['topic', 'relevant_text_span'])
            topics.append(Topic(i, text(row['topic'], nonempty=True), spans(row['relevant_text_span'], len(turns))))
        general = []
        for i, row in enumerate(sequence(data['general_query_list'], nonempty=True)):
            fields(row, ['query', 'answer']);general.append(GeneralQuery(i, text(row['query'], True), text(row['answer'], True)))
        specific = []
        for i, row in enumerate(sequence(data['specific_query_list'], nonempty=True)):
            fields(row, ['query', 'answer', 'relevant_text_span'])
            specific.append(SpecificQuery(i, text(row['query'], True), text(row['answer'], True), spans(row['relevant_text_span'], len(turns))))
        result.append(Meeting(ordinal, hashlib.sha256(line).hexdigest(), tuple(turns), tuple(topics), tuple(general), tuple(specific)))
    return tuple(result)


def load(root):
    raw = (root/'capture.json').read_bytes();capture = decode(raw)
    fields(capture, ['schema_version', 'repository', 'commit', 'tree', 'files', 'scope', 'validation_test_downloaded', 'inference_calls'])
    if (type(capture['schema_version']) is not int or capture['schema_version'] != 1 or capture['repository'] != REPOSITORY
            or capture['commit'] != COMMIT or capture['tree'] != TREE or capture['scope'] != 'author-training-topic-span-source-preflight'
            or capture['validation_test_downloaded'] is not False or type(capture['inference_calls']) is not int or capture['inference_calls'] != 0
            or type(capture['files']) is not dict or set(capture['files']) != set(FILES)):
        raise ValueError('pinned training-only author capture required')
    present = {str(p.relative_to(root)) for p in root.rglob('*') if p.is_file()}
    if present != set(FILES) | {'capture.json'}:raise ValueError('unexpected source files; held-out sources must remain absent')
    source = None
    for path, (blob, size) in FILES.items():
        target = root/path
        if target.is_symlink():raise ValueError('regular captured files required')
        content = target.read_bytes();pin = verify(content, blob, size)
        if capture['files'][path] != {'git_blob': blob, 'sha256': pin, 'bytes': size,
                                      'url': f'https://raw.githubusercontent.com/Yale-LILY/QMSum/{COMMIT}/{path}'}:
            raise ValueError('author capture provenance drift')
        if path == 'data/ALL/jsonl/train.jsonl':source = content
    if source is None:raise ValueError('complete original training source required')
    meetings = parse(source)
    if len(meetings) != 162:raise ValueError('complete original 162 training meetings required')
    return hashlib.sha256(raw).hexdigest(), hashlib.sha256(source).hexdigest(), meetings


def membership(meeting):
    counts = [Counter() for _ in meeting.turns]
    for topic in meeting.topics:
        for span in topic.spans:
            for i in range(span.start, span.end+1):counts[i][topic.ordinal] += 1
    return [tuple(sorted(c)) for c in counts], sum(n > 1 for c in counts for n in c.values())


def transition(previous, current):
    if not previous and not current:return Transition.UNKNOWN
    if not previous or not current:return Transition.GAP
    if previous == current:return Transition.STABLE
    if len(previous) == len(current) == 1:return Transition.SINGLETON_SWITCH
    return Transition.OVERLAP_CHANGE


def pin_text(value):
    return {'sha256': hashlib.sha256(value.encode()).hexdigest(), 'characters': len(value), 'utf8_bytes': len(value.encode())}


def span_records(value):
    return [{'ordinal': i, 'start': s.start, 'end_inclusive': s.end,
             'original_indices': [s.original_start, s.original_end]} for i, s in enumerate(value)]


def preflight(root):
    started = time.perf_counter();cpu = time.process_time();capture_pin, source_pin, meetings = load(root)
    ledger = [];total = Counter();transitions = Counter();turn_texts = Counter();topic_names = Counter()
    for meeting in meetings:
        members, duplicate_incidence = membership(meeting)
        local_transitions = [transition(members[i-1], members[i]) for i in range(1, len(members))]
        counts = {'turns': len(meeting.turns), 'topics': len(meeting.topics), 'topic_spans': sum(len(t.spans) for t in meeting.topics),
                  'recurring_topics': sum(len(t.spans) > 1 for t in meeting.topics),
                  'general_queries': len(meeting.general_queries), 'specific_queries': len(meeting.specific_queries),
                  'specific_query_spans': sum(len(q.spans) for q in meeting.specific_queries),
                  'unannotated_turns': sum(not m for m in members), 'multi_topic_turns': sum(len(m) > 1 for m in members),
                  'same_topic_multi_span_turn_incidence': duplicate_incidence,
                  'empty_turns': sum(not t.text.strip() for t in meeting.turns),
                  'empty_speakers': sum(not t.speaker.strip() for t in meeting.turns)}
        total.update(counts);transitions.update(local_transitions)
        turn_texts.update(t.text for t in meeting.turns);topic_names.update(t.name for t in meeting.topics)
        ledger.append({'ordinal': meeting.ordinal, 'raw_line_sha256': meeting.raw_line_sha256, 'counts': counts,
                       'turns': [{'ordinal': t.ordinal, 'speaker': pin_text(t.speaker), 'text': pin_text(t.text),
                                  'topic_ordinals': list(members[t.ordinal])} for t in meeting.turns],
                       'topics': [{'ordinal': t.ordinal, 'name': pin_text(t.name), 'spans': span_records(t.spans)} for t in meeting.topics],
                       'general_queries': [{'ordinal': q.ordinal, 'query': pin_text(q.query), 'answer': pin_text(q.answer)} for q in meeting.general_queries],
                       'specific_queries': [{'ordinal': q.ordinal, 'query': pin_text(q.query), 'answer': pin_text(q.answer),
                                             'spans': span_records(q.spans)} for q in meeting.specific_queries],
                       'transitions': [{'ordinal': i+1, 'kind': kind} for i, kind in enumerate(local_transitions)]})
    return {'schema_version': 1, 'scope': 'complete-training-topic-span-source-preflight-no-inference',
            'python': platform.python_version(), 'commit': COMMIT, 'capture_sha256': capture_pin, 'source_sha256': source_pin,
            'meetings': len(meetings), 'counts': dict(total), 'transitions': dict(transitions),
            'minimum_turns': min(len(m.turns) for m in meetings), 'maximum_turns': max(len(m.turns) for m in meetings),
            'maximum_turn_utf8_bytes': max(len(t.text.encode()) for m in meetings for t in m.turns),
            'exact_unique_turn_texts': len(turn_texts), 'repeated_turn_text_occurrences': sum(n-1 for n in turn_texts.values()),
            'exact_unique_topic_names': len(topic_names), 'ledger': ledger,
            'inference_calls': 0, 'validation_test_downloaded_or_scored': False,
            'code_pins': {n: hashlib.sha256(Path(__file__).with_name(n).read_bytes()).hexdigest()
                          for n in ['qmsum.py', 'qmsum_capture.py', 'tiage_capture.py', 'stage_parity.py']},
            'limitations': ['author marks main topics, not exhaustive fine-grained topic changes',
                           'annotation gaps are unknown, never automatic negative boundaries',
                           'multiple spans/overlaps do not supply ancestor hierarchy or universal topic IDs',
                           'meeting source lacks native IDs/domain metadata and agent/user roles',
                           'topic names, query text and answers are evaluation annotations, not model inputs',
                           'empty transcript turns remain at original ordinals',
                           'public annotation issues and upstream corpus licenses require separate review',
                           'meeting annotations are adjacent evidence, not coding-agent quality gold'],
            'timings': {'total_ms': (time.perf_counter()-started)*1000, 'process_cpu_ms': (time.process_time()-cpu)*1000,
                        'peak_rss_kib': resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}}


def main():
    p = argparse.ArgumentParser(description=__doc__);p.add_argument('--source', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True);p.add_argument('--compare', type=Path);a = p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    r = preflight(a.source)
    if a.compare:
        reference = decode(a.compare.read_bytes())
        # Only this driver's explicit top-level timing envelope may differ.
        # Source/model/schema fields never receive broad numeric coercion.
        if {k: v for k, v in reference.items() if k != 'timings'} != {k: v for k, v in r.items() if k != 'timings'}:
            raise ValueError('complete source preflight repeat differs')
    with a.output.open('x') as f:json.dump(r, f, indent=2, allow_nan=False)
    print(json.dumps({k: v for k, v in r.items() if k not in ['ledger', 'code_pins', 'limitations']}), flush=True)


if __name__ == '__main__':main()
