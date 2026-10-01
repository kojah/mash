"""Offline, source-pinned programming-question inventory; tags never enter inputs."""
import argparse
from collections import Counter
from dataclasses import dataclass
import html
import itertools
import json
from pathlib import Path
from urllib.parse import urlparse

from reference_review import digest


PINS = {
    'page-1.json': 'c4b9b5201844097667671199caec22b8437c614b7b1a9e46e440a1bfb629a3e6',
    'page-2.json': '076b0660cf9547154b4c9c68298ea3354bc27632ff720059a68aee3d571e5a18',
}
FROM = 1717200000
TO = 1719791999
SEED = 'mash-topic-stackoverflow-v1-20261002'
REQUESTS = {f'page-{page}.json':
            f'https://api.stackexchange.com/2.3/questions?site=stackoverflow&sort=creation&order=asc&fromdate={FROM}&todate={TO}&page={page}&pagesize=100&filter=withbody'
            for page in (1, 2)}


@dataclass(frozen=True)
class QuestionInput:
    id: int
    group: str
    title: str
    body_html: str
    source_sha256: str


@dataclass(frozen=True)
class TagReference:
    id: int
    tags: tuple[str, ...]
    link: str
    content_license: str | None
    creation_date: int


@dataclass(frozen=True)
class Corpus:
    inputs: tuple[QuestionInput, ...]
    references: tuple[TagReference, ...]


def encoded(value):
    return json.dumps(value, ensure_ascii=False, separators=(',', ':'), allow_nan=False).encode()


def integer(value, minimum=0):
    if type(value) is not int or value < minimum:
        raise ValueError('literal bounded integer required')
    return value


def text(value, cap):
    if not isinstance(value, str) or not value.strip() or len(value.encode()) > cap or '\x00' in value:
        raise ValueError('nonempty bounded source string required')
    return value


def parse(pages):
    inputs, references, seen, previous = [], [], set(), (0, 0)
    if not isinstance(pages, list) or len(pages) != 2:
        raise ValueError('exact two-page capture required')
    for page in pages:
        if not isinstance(page, dict) or any(k.startswith('error_') for k in page):
            raise ValueError('invalid or failed API response')
        if type(page.get('has_more')) is not bool:
            raise ValueError('literal pagination boolean required')
        for name in ('quota_max', 'quota_remaining'):
            integer(page.get(name))
        if 'backoff' in page:
            integer(page['backoff'], 1)
        rows = page.get('items')
        if not isinstance(rows, list) or len(rows) != 100:
            raise ValueError('100 questions per frozen page required; do not resample')
        for row in rows:
            if not isinstance(row, dict):
                raise ValueError('question object required')
            identity = integer(row.get('question_id'), 1)
            created = integer(row.get('creation_date'), FROM)
            if created > TO or identity in seen or (created, identity) < previous:
                raise ValueError('duplicate question, date range or creation order drift')
            previous = (created, identity)
            seen.add(identity)
            title, body = text(row.get('title'), 16384), text(row.get('body'), 512 * 1024)
            rendered_title = html.unescape(title)
            text(rendered_title, 16384)
            tags = row.get('tags')
            if (not isinstance(tags, list) or not 1 <= len(tags) <= 5
                    or any(not isinstance(t, str) or not t or len(t.encode()) > 256
                           or any(c.isspace() or ord(c) < 32 for c in t) for t in tags)
                    or len(set(tags)) != len(tags)):
                raise ValueError('one to five distinct literal source tags required')
            link = text(row.get('link'), 4096)
            url = urlparse(link)
            if url.scheme != 'https' or url.netloc != 'stackoverflow.com' or not url.path.startswith(f'/questions/{identity}/'):
                raise ValueError('question attribution link mismatch')
            license_name = row.get('content_license')
            if license_name is not None:
                text(license_name, 256)
            # Grouping depends only on source content, never tags/score/ownership.
            group = digest(encoded([rendered_title, body]))
            inputs.append(QuestionInput(identity, group, rendered_title, body, digest(encoded([title, body]))))
            references.append(TagReference(identity, tuple(sorted(tags)), link, license_name, created))
    if not pages[0]['has_more']:
        raise ValueError('second page contradicts first pagination state')
    return Corpus(tuple(inputs), tuple(references))


def load(root):
    pages = []
    for name, expected in PINS.items():
        path = root / name
        if not path.is_file() or path.stat().st_size > 16 * 1024 * 1024:
            raise ValueError('missing or oversized frozen source page')
        raw = path.read_bytes()
        if digest(raw) != expected:
            raise ValueError('frozen source page changed')
        pages.append(json.loads(raw))
    return parse(pages)


def split(corpus):
    representatives = {}
    for question in sorted(corpus.inputs, key=lambda q: q.id):
        representatives.setdefault(question.group, question)
    if len(representatives) < 128:
        raise ValueError('insufficient canonical questions; no replacement sampling')
    selected = sorted(representatives.values(), key=lambda q: (digest(SEED.encode() + bytes.fromhex(q.group)), q.group))
    return tuple(selected[:64]), tuple(selected[64:128]), tuple(selected[128:])


def inventory(root):
    corpus = load(root)
    references = {r.id: r for r in corpus.references}
    calibration, reserved, background = split(corpus)
    groups = {}
    for q in corpus.inputs:
        groups.setdefault(q.group, []).append(q.id)
    # Conflicting alias tags are exposed; never merged into extra model features.
    conflicts = sum(len({references[i].tags for i in aliases}) > 1 for aliases in groups.values())
    phases = {}
    for name, qs in [('calibration', calibration), ('reserved', reserved), ('background', background)]:
        pairs = list(itertools.combinations(qs, 2))
        overlap = sum(bool(set(references[a.id].tags) & set(references[b.id].tags)) for a, b in pairs)
        frequency = Counter(t for q in qs for t in references[q.id].tags)
        phases[name] = {'questions': [{'id': q.id, 'group': q.group, 'source_sha256': q.source_sha256} for q in qs],
                        'tag_document_frequency': dict(sorted(frequency.items())), 'pairs': len(pairs),
                        'tag_overlap_pairs': overlap, 'no_tag_overlap_pairs': len(pairs) - overlap,
                        'all_connected_precision': overlap / len(pairs) if pairs else None,
                        'same_decoded_title_pairs': sum(a.title == b.title for a, b in pairs)}
    return {'version': 1, 'scope': 'programming-question-source-inventory-not-model-quality',
            'policy': {'seed': SEED, 'requests': REQUESTS, 'source_pins': PINS,
                       'group': 'sha256-compact-utf8-json-decoded-title-original-body-html',
                       'split': 'first64-calibration-next64-reserved-remainder-background',
                       'labels': 'all-source-tags-evaluation-only-no-synonym-or-language-removal'},
            'code_pins': {n: digest(Path(__file__).with_name(n).read_bytes()) for n in
                          ('stackoverflow_source.py', 'reference_review.py')},
            'source_questions': len(corpus.inputs), 'canonical_groups': len(groups),
            'duplicate_groups': sum(len(ids) > 1 for ids in groups.values()), 'alias_tag_conflicts': conflicts,
            'license_counts': dict(Counter(r.content_license or 'unknown' for r in corpus.references)),
            'creation_range': [min(r.creation_date for r in corpus.references), max(r.creation_date for r in corpus.references)],
            'tag_count_distribution': {str(n): count for n, count in sorted(Counter(len(r.tags) for r in corpus.references).items())},
            'phases': phases}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--compare', type=Path)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git output required')
    result = inventory(args.root)
    if args.compare and json.loads(args.compare.read_text()) != json.loads(json.dumps(result)):
        raise ValueError('source inventory repeat drift')
    with args.output.open('x') as output:
        json.dump(result, output, sort_keys=True, allow_nan=False)
    print(json.dumps({k: result[k] for k in ('source_questions', 'canonical_groups', 'duplicate_groups', 'alias_tag_conflicts', 'license_counts')}))


if __name__ == '__main__':
    main()
