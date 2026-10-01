"""Pinned external development corpus; model text is separate from annotation metadata."""
import argparse
from collections import Counter
from dataclasses import dataclass
from enum import StrEnum
import hashlib
import json
from pathlib import Path

from reference_review import digest, exact_keys


COMMIT = 'fe0c8e65cfcd8462bd33c86e35f21addc84ca82b'
PINS = {'README.md': 'a9b2c9bb4ca61fdf1eb64a3c5d421271b14ffcc45029852fd26c71ca32302c9e',
        'schema.json': 'ae9e2390f38fb967af64623c2f4f7e0c636fb377ad523b582a03161d3ddbdf68',
        'dev/dialogues_001.json': 'e7ddb563e4da5766ea820cc826dead77e7ca219c19b761e218d62d9c999a252e'}


class Service(StrEnum):
    HOTEL = 'hotel'
    TRAIN = 'train'
    ATTRACTION = 'attraction'
    RESTAURANT = 'restaurant'
    HOSPITAL = 'hospital'
    TAXI = 'taxi'
    BUS = 'bus'
    POLICE = 'police'


class Speaker(StrEnum):
    USER = 'USER'
    SYSTEM = 'SYSTEM'


@dataclass(frozen=True)
class Schema:
    service: Service
    slots: frozenset[str]
    intents: frozenset[str]


@dataclass(frozen=True)
class Turn:
    dialogue: str
    ordinal: int
    speaker: Speaker
    text: str


@dataclass(frozen=True)
class Dialogue:
    id: str
    group: str
    turns: tuple[Turn, ...]


@dataclass(frozen=True)
class Label:
    dialogue: str
    ordinal: int
    active: tuple[Service, ...]
    unknown_intents: tuple[tuple[Service, str], ...]


@dataclass(frozen=True)
class ValueSpan:
    dialogue: str
    ordinal: int
    slot: str
    start: int
    end: int
    byte_start: int
    byte_end: int
    text_sha256: str


@dataclass(frozen=True)
class Copy:
    dialogue: str
    ordinal: int
    slot: str
    source_slot: str
    source_values_equal: bool


@dataclass(frozen=True)
class Corpus:
    dialogues: tuple[Dialogue, ...]
    labels: tuple[Label, ...]
    spans: tuple[ValueSpan, ...]
    copies: tuple[Copy, ...]


def strings(value):
    if not isinstance(value, list) or any(not isinstance(s, str) for s in value):
        raise ValueError('expected literal string list')
    return tuple(value)


def schemas(rows):
    result = {}
    for row in rows:
        exact_keys(row, {'service_name', 'slots', 'description', 'intents'})
        service = Service(row['service_name'])
        if service in result or not isinstance(row['description'], str):
            raise ValueError('duplicate or invalid service schema')
        slots, intents = set(), set()
        for slot in row['slots']:
            # The pinned schema omits possible_values for noncategorical slots.
            exact_keys(slot, {'name', 'description', 'is_categorical',
                              *({'possible_values'} if 'possible_values' in slot else set())})
            if (not isinstance(slot['name'], str) or slot['name'] in slots or not slot['name'].startswith(service + '-')
                    or type(slot['is_categorical']) is not bool or not isinstance(slot['description'], str)):
                raise ValueError('invalid schema slot')
            if 'possible_values' in slot:
                strings(slot['possible_values'])
            elif slot['is_categorical']:
                raise ValueError('categorical slot lacks declared values')
            slots.add(slot['name'])
        for intent in row['intents']:
            exact_keys(intent, {'name', 'description', 'is_transactional', 'required_slots', 'optional_slots'})
            if (not isinstance(intent['name'], str) or not intent['name'] or intent['name'] in intents
                    or type(intent['is_transactional']) is not bool or not isinstance(intent['description'], str)
                    or not isinstance(intent['optional_slots'], dict)
                    or not set(strings(intent['required_slots'])) <= slots
                    or not set(intent['optional_slots']) <= slots):
                raise ValueError('invalid schema intent')
            intents.add(intent['name'])
        result[service] = Schema(service, frozenset(slots), frozenset(intents))
    if not result:
        raise ValueError('empty external schema')
    return result


def parse(schema_rows, rows):
    """Fail on source-shape corruption; preserve undeclared intents and copy differences."""
    schema = schemas(schema_rows)
    all_slots = set().union(*(s.slots for s in schema.values()))
    dialogues, labels, spans, copies, seen = [], [], [], [], set()
    for row in rows:
        exact_keys(row, {'dialogue_id', 'services', 'turns'})
        identity = row['dialogue_id']
        if not isinstance(identity, str) or not identity or identity in seen:
            raise ValueError('duplicate or empty dialogue identity')
        seen.add(identity)
        services = tuple(Service(s) for s in strings(row['services']))
        if len(set(services)) != len(services) or not set(services) <= set(schema):
            raise ValueError('invalid dialogue services')
        if not 2 <= len(row['turns']) <= 256 or len(row['turns']) % 2:
            raise ValueError('expected complete bounded alternating dialogue')
        turns = []
        for ordinal, raw in enumerate(row['turns']):
            exact_keys(raw, {'turn_id', 'speaker', 'utterance', 'frames'})
            speaker = Speaker(raw['speaker'])
            if raw['turn_id'] != str(ordinal) or speaker != (Speaker.USER if ordinal % 2 == 0 else Speaker.SYSTEM):
                raise ValueError('source turn identity or role order changed')
            text = raw['utterance']
            if not isinstance(text, str) or not text.strip() or len(text.encode()) > 65536:
                raise ValueError('invalid or oversized original utterance')
            turns.append(Turn(identity, ordinal, speaker, text))
            frame_seen, active, unknown, states = set(), [], [], {}
            for frame in raw['frames']:
                exact_keys(frame, {'service', 'actions', 'slots', *({'state'} if speaker == Speaker.USER else set())})
                service = Service(frame['service'])
                if service in frame_seen or service not in schema or frame['actions'] != []:
                    raise ValueError('duplicate/unknown frame or unexpected action payload')
                frame_seen.add(service)
                if speaker == Speaker.USER:
                    state = frame['state']
                    exact_keys(state, {'active_intent', 'requested_slots', 'slot_values'})
                    if (not isinstance(state['active_intent'], str) or not isinstance(state['slot_values'], dict)
                            or not set(strings(state['requested_slots'])) <= schema[service].slots
                            or not set(state['slot_values']) <= schema[service].slots):
                        raise ValueError('invalid external user state')
                    for slot, values in state['slot_values'].items():
                        states[slot] = strings(values)
                    intent = state['active_intent']
                    if intent != 'NONE':
                        if intent in schema[service].intents:
                            active.append(service)
                        else:
                            unknown.append((service, intent))
            if speaker == Speaker.USER:
                if frame_seen != set(schema):
                    raise ValueError('incomplete source user frame coverage')
                labels.append(Label(identity, ordinal, tuple(sorted(active)), tuple(sorted(unknown))))
            for frame in raw['frames']:
                service = Service(frame['service'])
                for slot in frame['slots']:
                    if slot['slot'] not in schema[service].slots:
                        raise ValueError('span slot absent from source schema')
                    if 'copy_from' in slot:
                        exact_keys(slot, {'slot', 'copy_from', 'value'})
                        values = strings(slot['value'])
                        if speaker != Speaker.USER or slot['copy_from'] not in all_slots or values != states.get(slot['slot']):
                            raise ValueError('copy target/source identity or current values changed')
                        copies.append(Copy(identity, ordinal, slot['slot'], slot['copy_from'], values == states.get(slot['copy_from'])))
                    else:
                        exact_keys(slot, {'slot', 'start', 'exclusive_end', 'value'})
                        start, end = slot['start'], slot['exclusive_end']
                        if (type(start) is not int or type(end) is not int or not 0 <= start < end <= len(text)
                                or text[start:end] != slot['value']):
                            raise ValueError('span bounds/value do not match exact utterance')
                        spans.append(ValueSpan(identity, ordinal, slot['slot'], start, end,
                                               len(text[:start].encode()), len(text[:end].encode()), digest(text[start:end].encode())))
        hashed = hashlib.sha256()
        for turn in turns:
            for value in (turn.speaker.encode(), turn.text.encode()):
                hashed.update(len(value).to_bytes(8, 'little'))
                hashed.update(value)
        dialogues.append(Dialogue(identity, hashed.hexdigest(), tuple(turns)))
    if not 1 <= len(dialogues) <= 1024 or sum(len(d.turns) for d in dialogues) > 32768:
        raise ValueError('external dialogue count exceeds development cap')
    return Corpus(tuple(dialogues), tuple(labels), tuple(spans), tuple(copies))


def load(root):
    """Read only this pinned development shard; no network or train/test fallback."""
    base = root / 'data/MultiWOZ_2.2'
    raw = {}
    for name, expected in PINS.items():
        path = base / name
        if path.stat().st_size > 16 * 1024 * 1024:
            raise ValueError('external source exceeds byte cap')
        raw[name] = path.read_bytes()
        if digest(raw[name]) != expected:
            raise ValueError('pinned external source changed')
    return parse(json.loads(raw['schema.json']), json.loads(raw['dev/dialogues_001.json']))


def select(corpus, seed, maximum=32):
    """Canonical whole-text groups only; no service/intent names used for sampling."""
    if not isinstance(seed, str) or not seed or type(maximum) is not int or not 1 <= maximum <= 32:
        raise ValueError('bounded source selection required')
    labels = {(l.dialogue, l.ordinal): (l.active, l.unknown_intents) for l in corpus.labels}
    groups, duplicates = {}, 0
    for dialogue in sorted(corpus.dialogues, key=lambda d: d.id):
        if dialogue.group in groups:
            previous = groups[dialogue.group]
            if [labels[t.dialogue, t.ordinal] for t in dialogue.turns if t.speaker == Speaker.USER] != [
                    labels[t.dialogue, t.ordinal] for t in previous.turns if t.speaker == Speaker.USER]:
                raise ValueError('identical text aliases have conflicting label proxies')
            duplicates += 1
        else:
            groups[dialogue.group] = dialogue
    ordered = sorted(groups.values(), key=lambda d: (digest((seed + ':' + d.group).encode()), d.group))
    if len(ordered) < maximum:
        raise ValueError('not enough canonical development groups')
    return tuple(ordered[:maximum]), duplicates


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    args = parser.parse_args()
    corpus = load(args.root)
    print(json.dumps({'dialogues': len(corpus.dialogues), 'turns': sum(len(d.turns) for d in corpus.dialogues),
                      'value_spans': len(corpus.spans), 'copies': len(corpus.copies),
                      'copy_source_alias_differences': sum(not c.source_values_equal for c in corpus.copies),
                      'undeclared_intents': dict(Counter(i for l in corpus.labels for _, i in l.unknown_intents)),
                      'inference_calls': 0}))


if __name__ == '__main__':
    main()
