"""Sparse source-adjacent class labels; independent of query/reference outcomes."""
from dataclasses import dataclass
import re

import numpy as np
from scipy import sparse
from sklearn.feature_extraction.text import CountVectorizer

from subject_labels import ranked_terms


TOKEN = re.compile(r'(?u)(?<!\w)\w[\w.+#:/-]*')


@dataclass(frozen=True)
class Term:
    value: str
    start: int
    end: int


def source_terms(text):
    previous = None
    for match in TOKEN.finditer(text):
        raw = match.group().rstrip('.:/-')
        if not raw or len(raw) > 128:
            previous = None
            continue
        term = Term(raw.casefold(), match.start(), match.start()+len(raw))
        yield term
        if previous is not None and text[previous.end:term.start].isspace():
            yield Term(previous.value+' '+term.value, previous.start, term.end)
        previous = term


def analyzer(text):
    return [term.value for term in source_terms(text)]


def vocabulary(documents):
    if not documents or len(documents) > 100000:
        raise ValueError('bounded complete source corpus required')
    # NUL separates original fields; it is not whitespace, so cannot form a phrase.
    vectorizer = CountVectorizer(analyzer=analyzer, min_df=2, max_df=.4,
                                 max_features=32768, dtype=np.float64)
    counts = vectorizer.fit_transform(doc.title+'\0'+doc.text for doc in documents).tocsr()
    counts.sum_duplicates()
    return vectorizer.get_feature_names_out().tolist(), counts


def classes(counts, members, topics):
    if counts.shape[0] != len(members) or not 1 <= topics <= 8192:
        raise ValueError('aligned sparse source counts/memberships required')
    rows, columns, weights = [], [], []
    for doc, matches in enumerate(members):
        if len(matches) > 3 or len({m.topic for m in matches}) != len(matches):
            raise ValueError('unique bounded topic memberships required')
        for match in matches:
            if not 0 <= match.topic < topics:
                raise ValueError('known topic required')
            rows.append(match.topic); columns.append(doc); weights.append(1/len(matches))
    incidence = sparse.csr_matrix((weights, (rows, columns)), shape=(topics, len(members)))
    aggregated = (incidence @ counts).tocsr()
    aggregated.eliminate_zeros()
    if aggregated.nnz > 8_000_000:
        raise ValueError('sparse label budget exceeded; do not truncate')
    return aggregated


def scores(class_counts, background):
    if class_counts.shape[1] != background.shape[1] or (class_counts.data < 0).any() or not np.isfinite(class_counts.data).all():
        raise ValueError('finite nonnegative aligned sparse labels required')
    lengths = np.asarray(class_counts.sum(axis=1)).ravel()
    frequency = np.asarray(class_counts.sum(axis=0)).ravel()
    rows = np.repeat(np.arange(len(lengths)), np.diff(class_counts.indptr))
    base = class_counts.copy()
    base.data = base.data/np.maximum(lengths[rows], 1)*np.log1p(lengths.mean()/frequency[base.indices])
    df = np.asarray((background > 0).sum(axis=0)).ravel()
    idf = np.log((1+background.shape[0])/(1+df))+1
    contrastive = base.copy()
    contrastive.data *= idf[contrastive.indices]
    return base, contrastive


def labels(class_counts, background, terms):
    base, contrastive = scores(class_counts, background)
    if len(terms) != base.shape[1] or terms != sorted(set(terms)):
        raise ValueError('canonical complete vocabulary required')
    result = []
    for row in range(base.shape[0]):
        a, b = base.getrow(row), contrastive.getrow(row)
        assert np.array_equal(a.indices, b.indices)
        local_terms = [terms[i] for i in a.indices]
        result.append({'ctfidf': ranked_terms(a.data, local_terms),
                       'corpus-idf': ranked_terms(b.data, local_terms),
                       'corpus-idf-diverse': ranked_terms(b.data, local_terms, diverse=True)})
    return result


def support(document, term):
    for field, text in [('title', document.title), ('body', document.text)]:
        for candidate in source_terms(text):
            if candidate.value == term:
                return {'document': document.id, 'field': field,
                        'start': candidate.start, 'end': candidate.end,
                        'byte_start': len(text[:candidate.start].encode()),
                        'byte_end': len(text[:candidate.end].encode()),
                        'text': text[candidate.start:candidate.end],
                        'source_sha256': document.content_sha256}
    raise ValueError('label lacks exact original source support')
