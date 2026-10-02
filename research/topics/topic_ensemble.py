"""Incremental dense corroboration of literal phrase proposals; research only."""
from collections import defaultdict
import copy
import numpy as np
from topic_features import terms
from topic_stream import Source


def direction(values):
    norm = float(np.linalg.norm(values))
    return values / norm if norm else None


class Corroborator:
    def __init__(self):
        self.records = {}
        self.units = defaultdict(set)
        self.buckets = defaultdict(dict)
        self.directions = {}

    def _refresh(self, term, thread):
        key = (term, thread)
        bucket = self.buckets[key]
        if not bucket:
            del self.buckets[key]
            self.directions.pop(key, None)
        else:
            self.directions[key] = direction(sum(
                (bucket[text] for text in sorted(bucket)),
                np.zeros_like(next(iter(bucket.values())))))

    def upsert(self, source):
        if (not isinstance(source, Source)
                or any(not isinstance(v, str) or not v or len(v) > 4096
                       for v in [source.id, source.thread, source.provider])
                or not isinstance(source.text, str) or len(source.text.encode()) > 262144):
            raise ValueError('bounded typed source required')
        vector = np.asarray(source.vector, dtype=np.float64)
        if vector.ndim != 1 or not 1 <= len(vector) <= 4096 or not np.isfinite(vector).all():
            raise ValueError('finite nonempty vector required')
        with np.errstate(over='ignore'):
            norm = float(np.linalg.norm(vector))
        if not np.isfinite(norm):
            raise ValueError('finite vector norm required')
        prior = self.records.get(source.id)
        if prior is None and len(self.records) >= 2048:
            raise ValueError('source capacity exceeded')
        total = sum(len(row.text.encode()) for row in self.records.values())
        if total - (len(prior.text.encode()) if prior else 0) + len(source.text.encode()) > 8388608:
            raise ValueError('text capacity exceeded')
        for identity, row in self.records.items():
            if identity == source.id:
                continue
            if len(row.vector) != len(vector):
                raise ValueError('embedding dimensions differ')
            if row.thread == source.thread and row.provider != source.provider:
                raise ValueError('thread provider scope differs')
            if row.thread == source.thread and row.text == source.text and not np.array_equal(row.vector, vector):
                raise ValueError('conflicting duplicate geometry')
        if prior is not None and (prior.thread, prior.provider, prior.text) == (source.thread, source.provider, source.text) and np.array_equal(prior.vector, vector):
            return False
        self.delete(source.id)
        row = Source(source.id, source.thread, source.provider, source.text, vector.copy())
        self.records[row.id] = row
        key = (row.thread, row.text)
        first = not self.units[key]
        self.units[key].add(row.id)
        if first:
            unit = direction(vector)
            if unit is None:
                unit = np.zeros_like(vector)
            for term in set(terms(row.text)):
                self.buckets[term, row.thread][row.text] = unit
                self._refresh(term, row.thread)
        return True

    def delete(self, identity):
        row = self.records.pop(identity, None)
        if row is None:
            return False
        key = (row.thread, row.text)
        self.units[key].remove(identity)
        if not self.units[key]:
            del self.units[key]
            for term in set(terms(row.text)):
                del self.buckets[term, row.thread][row.text]
                self._refresh(term, row.thread)
        return True

    def judge(self, term, thread):
        keys = sorted(key for key in self.buckets if key[0] == term)
        inputs = {text for key in keys for text in self.buckets[key]}
        if (term, thread) not in self.buckets:
            raise ValueError('proposal lacks literal evidence')
        if len(keys) < 2 or len(inputs) < 2:
            return {'keep': True, 'state': 'provisional', 'cosine': None}
        local = self.directions[term, thread]
        peers = [self.directions[key] for key in keys if key[1] != thread]
        if local is None or any(peer is None for peer in peers):
            return {'keep': False, 'state': 'undefined_direction', 'cosine': None}
        peer = direction(sum(peers, np.zeros_like(local)))
        if peer is None:
            return {'keep': False, 'state': 'undefined_direction', 'cosine': None}
        score = float(np.dot(local, peer))
        return {'keep': score >= .55, 'state': 'corroborated' if score >= .55 else 'uncorroborated', 'cosine': score}

    def checkpoint(self):
        return [{'id': row.id, 'thread': row.thread, 'provider': row.provider,
                 'text': row.text, 'vector': row.vector.tolist()}
                for _, row in sorted(self.records.items())]

    @classmethod
    def restore(cls, value):
        if not isinstance(value, list) or len(value) > 2048:
            raise ValueError('bounded checkpoint required')
        model = cls()
        for row in value:
            if not isinstance(row, dict) or set(row) != {'id', 'thread', 'provider', 'text', 'vector'} or not isinstance(row['id'], str) or row['id'] in model.records:
                raise ValueError('unique typed checkpoint rows required')
            model.upsert(Source(**row))
        return model

    def apply(self, snapshot):
        result = copy.deepcopy(snapshot)
        topics = {topic['id']: topic for topic in result['topics']}
        source_threads = {sid: row['thread'] for row in result['threads'] for sid in row['sources']}
        decisions = []
        allowed = set()
        for row in result['threads']:
            kept = []
            for tag in row['tags']:
                decision = self.judge(topics[tag['topic']]['phrase'], row['thread'])
                decisions.append({'thread': row['thread'], 'topic': tag['topic'], **decision})
                if decision['keep']:
                    kept.append(tag)
                    allowed.add((row['thread'], tag['topic']))
            row['tags'] = kept
        for sid, tags in result['memberships'].items():
            result['memberships'][sid] = [tag for tag in tags if (source_threads[sid], tag['topic']) in allowed]
        for topic in result['topics']:
            topic['threads'] = [thread for thread in topic['threads'] if (thread, topic['id']) in allowed]
            topic['sources'] = [sid for sid in topic['sources'] if (source_threads[sid], topic['id']) in allowed]
        result['topics'] = [topic for topic in result['topics'] if topic['threads']]
        result['representation'] = 'phrase_dense_corroboration'
        result['parameters']['dense_corroboration'] = 'other-thread normalized mean cosine >= .55; provisional retained'
        result['diagnostics'].update(topics=len(result['topics']),
            recurring_topics=sum(t['support'] == 'cross_thread_and_distinct_input' for t in result['topics']),
            provisional_topics=sum(t['support'] == 'provisional' for t in result['topics']),
            untagged_sources=sum(not tags for tags in result['memberships'].values()),
            multi_tag_sources=sum(len(tags) > 1 for tags in result['memberships'].values()),
            untagged_threads=sum(not row['tags'] for row in result['threads']))
        return result, decisions
