"""Source-only lexical features and fixed label budgets for question-unit topics."""
from collections import defaultdict
from dataclasses import dataclass

import numpy as np
from sklearn.feature_extraction.text import CountVectorizer

from phrase_labels import source_terms, supported
from question_html import render
from reference_review import digest
from subject_labels import label_scores, ranked_terms


@dataclass
class Features:
    terms: list[str]
    matrix: object
    positions: dict[str, int]
    texts: dict[str, str]
    unit_sources: dict[str, tuple[str, ...]]
    parent_sources: dict[str, tuple[str, ...]]
    unit_parents: dict[str, str]
    background: np.ndarray


def features(selected, background, capture):
    texts, unit_sources, unit_parents, parents = {}, {}, {}, defaultdict(list)
    for q in selected:
        identity = str(q.id)
        title = 'title/' + identity
        texts[title] = q.title
        parents[identity].append(title)
        body = render(q.body_html).text
        plan = capture['plans'][identity]
        if plan['reason'] is not None or plan['source_sha256'] != digest(q.body_html.encode()) or plan['displayed_sha256'] != digest(body.encode()):
            raise ValueError('complete frozen question source required')
        cursor = byte_cursor = 0
        hashes = []
        for u in plan['units']:
            piece = body[u['start']:u['end']]
            if (u['start'] != cursor or u['byte_start'] != byte_cursor or u['end'] <= u['start']
                    or piece.encode() != body.encode()[u['byte_start']:u['byte_end']]
                    or u['body_sha256'] != digest(piece.encode())
                    or u['input_sha256'] != digest((q.title + '\n\n' + piece).encode())):
                raise ValueError('original unit source/offset/input changed')
            cursor, byte_cursor = u['end'], u['byte_end']
            if u['reason'] is not None:
                if u['reason'] != 'whitespace-only' or piece.strip():
                    raise ValueError('nonempty unit omitted')
                continue
            hashes.append(u['input_sha256'])
            key = identity + '/' + u['input_sha256']
            source = 'body/' + key
            texts[source] = piece
            unit_sources[key], unit_parents[key] = (title, source), identity
            if source not in parents[identity]:
                parents[identity].append(source)
        if cursor != len(body) or byte_cursor != len(body.encode()) or hashes != capture['input_hashes'][identity]:
            raise ValueError('incomplete original source unit plan')
    for q in background:
        texts['background-title/' + str(q.id)] = q.title
        texts['background-body/' + str(q.id)] = render(q.body_html).text
    keys = sorted(texts)
    vectorizer = CountVectorizer(analyzer=source_terms, max_features=8192)
    matrix = vectorizer.fit_transform(texts[k] for k in keys).tocsr()
    positions = {key: n for n, key in enumerate(keys)}
    # One DF vote/question, combining separately counted title and body records.
    bg = np.vstack([np.asarray(matrix[[positions['background-title/' + str(q.id)],
                                      positions['background-body/' + str(q.id)]]].sum(0)).ravel() for q in background])
    return Features(vectorizer.get_feature_names_out().tolist(), matrix, positions, texts,
                    unit_sources, {k: tuple(v) for k, v in parents.items()}, unit_parents, bg)


def topic_labels(groups, source_map, memberships, features):
    sources = [sorted({key for point in group for key in source_map[point]}) for group in groups]
    counts = np.vstack([np.asarray(features.matrix[[features.positions[k] for k in keys]].sum(0)).ravel()
                        for keys in sources])
    baseline, contrastive = label_scores(counts, features.background)
    scorers = {'ctfidf': baseline, 'background-idf': contrastive}
    clusters = []
    for n, keys in enumerate(sources):
        names = {method: ranked_terms(scores[n], features.terms) for method, scores in scorers.items()}
        if any(not supported(label['term'], [features.texts[k] for k in keys]) for values in names.values() for label in values):
            raise ValueError('label is not supported by original topic source')
        clusters.append({'sources': keys, 'labels': names})
    parent_labels = {parent: {method: ranked_terms(scores[sorted(tags)].max(0), features.terms)
                              for method, scores in scorers.items()} for parent, tags in memberships.items()}
    return {'clusters': clusters, 'parent_top8': parent_labels,
            'counts_sha256': digest(np.asarray(counts, dtype='<i8').tobytes()), 'counts_shape': list(counts.shape)}
