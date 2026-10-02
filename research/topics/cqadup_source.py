"""Strict source/reference separation and duplicate-family query partitioning."""
import csv
from dataclasses import dataclass
import hashlib
import io
import json
from pathlib import Path

from stackoverflow_source import encoded
from topic_review import decode


SEED = 'mash-cqadup-programmers-v1-20261002'


@dataclass(frozen=True)
class Source:
    id: str
    title: str
    text: str
    content_sha256: str


@dataclass(frozen=True)
class Reference:
    query: str
    document: str
    relevance: int


def parse_source(value, query=False):
    required = {'_id', 'text', 'metadata'} if query else {'_id', 'title', 'text', 'metadata'}
    if not isinstance(value, dict) or set(value) != required or not isinstance(value['metadata'], dict):
        raise ValueError('exact BEIR source schema and opaque metadata required')
    identity, title, text = value['_id'], '' if query else value['title'], value['text']
    if (not isinstance(identity, str) or not identity.strip() or len(identity) > 128
            or not isinstance(title, str) or not isinstance(text, str) or not (title.strip() or text.strip())
            or len(title.encode()) + len(text.encode()) > 65536):
        raise ValueError('bounded nonempty identity and complete text fields required')
    return Source(identity, title, text, hashlib.sha256(encoded([title, text])).hexdigest())


def parse_references(raw, queries, documents):
    reader = csv.reader(io.StringIO(raw.decode()), delimiter='\t')
    if next(reader, None) != ['query-id', 'corpus-id', 'score']:
        raise ValueError('exact qrels header required')
    output, seen = [], set()
    for row in reader:
        if len(row) != 3 or row[0] not in queries or row[1] not in documents or row[2] != '1':
            raise ValueError('known query/document and binary positive reference required')
        key = row[0], row[1]
        if key in seen:
            raise ValueError('duplicate reference pair')
        seen.add(key)
        output.append(Reference(*key, 1))
    if not output:
        raise ValueError('nonempty reference population required')
    return tuple(output)


def load(root, capture_pin):
    raw = (root / 'capture.json').read_bytes()
    if hashlib.sha256(raw).hexdigest() != capture_pin:
        raise ValueError('pinned complete capture manifest required')
    capture = decode(raw)
    if not capture['member_crc_verified'] or set(capture['files']) != {'corpus.jsonl', 'queries.jsonl', 'qrels.tsv'}:
        raise ValueError('complete CRC-verified source capture required')
    fields, summaries = {}, {}
    for name, query, cap in (('corpus.jsonl', False, 100000), ('queries.jsonl', True, 5000)):
        sources, sha, total, max_line = {}, hashlib.sha256(), 0, 0
        with (root / name).open('rb') as file:
            while raw := file.readline(4 * 1024 * 1024 + 1):
                if len(raw) > 4 * 1024 * 1024 or len(sources) >= cap:
                    raise ValueError('source record/line budget exceeded; do not truncate')
                sha.update(raw); total += len(raw); max_line = max(max_line, len(raw))
                source = parse_source(decode(raw), query)
                if source.id in sources:
                    raise ValueError('unique source IDs required')
                sources[source.id] = source
        pin = capture['files'][name]
        if sha.hexdigest() != pin['sha256'] or total != pin['size'] or not sources:
            raise ValueError('complete pinned source bytes required')
        fields[name] = sources
        summaries[name] = {'records': len(sources), 'raw_bytes': total, 'max_line_bytes': max_line,
                           'feature_text_bytes': sum(len(s.title.encode()) + len(s.text.encode()) for s in sources.values()),
                           'max_feature_text_bytes': max(len(s.title.encode()) + len(s.text.encode()) for s in sources.values())}
    refs_raw = (root / 'qrels.tsv').read_bytes()
    pin = capture['files']['qrels.tsv']
    if hashlib.sha256(refs_raw).hexdigest() != pin['sha256'] or len(refs_raw) != pin['size']:
        raise ValueError('pinned complete qrels required')
    documents, queries = fields['corpus.jsonl'], fields['queries.jsonl']
    references = parse_references(refs_raw, queries, documents)
    return documents, queries, references, summaries


def partition(documents, queries, references, development=64, reserved=64):
    if (type(development) is not int or type(reserved) is not int or not 1 <= development <= 128
            or not 1 <= reserved <= 128):
        raise ValueError('bounded positive component budgets required')
    for records in (documents, queries):
        if any(key != source.id or source.content_sha256 != hashlib.sha256(encoded([source.title, source.text])).hexdigest()
               for key, source in records.items()):
            raise ValueError('canonical source keys and content hashes required')
    nodes = {'d/' + s.id: s for s in documents.values()} | {'q/' + s.id: s for s in queries.values()}
    if len(nodes) != len(documents) + len(queries):
        raise ValueError('unique source node identities required')
    parents = {key: key for key in nodes}
    def find(node):
        while parents[node] != node:
            parents[node] = parents[parents[node]]
            node = parents[node]
        return node
    def union(a, b):
        x, y = find(a), find(b)
        if x != y:
            parents[max(x, y)] = min(x, y)
    for ref in references:
        if type(ref.relevance) is not int or ref.relevance != 1 or 'q/' + ref.query not in nodes or 'd/' + ref.document not in nodes:
            raise ValueError('known binary positive reference required')
        union('q/' + ref.query, 'd/' + ref.document)
    for identity in set(documents) & set(queries):
        union('d/' + identity, 'q/' + identity)
    by_content = {}
    for node, source in sorted(nodes.items()):
        previous = by_content.setdefault(source.content_sha256, node)
        union(previous, node)
    components = {}
    for node in sorted(nodes):
        components.setdefault(find(node), []).append(node)
    groups = []
    for members in components.values():
        question_ids = [n[2:] for n in members if n.startswith('q/')]
        if question_ids:
            identity = hashlib.sha256(encoded(members)).hexdigest()
            rank = hashlib.sha256(encoded([SEED, identity])).hexdigest()
            groups.append({'id': identity, 'members': members, 'queries': question_ids, 'rank': rank})
    groups.sort(key=lambda g: (g['rank'], g['id']))
    if len(groups) < development + reserved:
        raise ValueError('insufficient distinct query families; do not refill or shrink silently')
    phases = {'development': groups[:development], 'reserved': groups[development:development + reserved],
              'unused': groups[development + reserved:]}
    return {'seed': SEED, 'policy': 'qrel-connected-plus-identical-id/content-groups-no-new-relevance',
            'phases': phases, 'query_families': len(groups), 'all_components': len(components),
            'shared_query_corpus_ids': sorted(set(documents) & set(queries)),
            'summary': {name: {'families': len(group), 'queries': sum(len(g['queries']) for g in group),
                               'largest_family_nodes': max((len(g['members']) for g in group), default=0)}
                        for name, group in phases.items()}}
