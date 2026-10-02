"""Fixed sparse lexical controls; reference labels never enter fitting/scoring."""
from dataclasses import dataclass
from enum import StrEnum
import math

import numpy as np
from sklearn.feature_extraction.text import CountVectorizer, TfidfTransformer


class Method(StrEnum):
    WORD = 'word-tfidf'
    CHARACTER = 'character-tfidf'
    BM25 = 'word-bm25'


@dataclass
class Index:
    method: Method
    vectorizer: CountVectorizer
    counts: object
    weights: object
    transformer: object | None
    lengths: np.ndarray
    idf: np.ndarray

    def score(self, query):
        if not isinstance(query, str) or len(query.encode()) > 65536:
            raise ValueError('bounded query text required')
        counts = self.vectorizer.transform([query])
        if self.transformer is not None:
            query_weights = self.transformer.transform(counts)
        else:
            query_weights = counts.copy()
            query_weights.data[:] = 1
        scores = (self.weights @ query_weights.T).toarray().ravel()
        if not np.isfinite(scores).all() or (scores < 0).any():
            raise ValueError('finite nonnegative complete scores required')
        return scores, int(counts.nnz)


def fit(texts, method):
    method = Method(method)
    if (not 2 <= len(texts) <= 100000 or any(not isinstance(t, str) or len(t.encode()) > 65537 for t in texts)):
        raise ValueError('bounded complete corpus required')
    kwargs = {'min_df': 2, 'max_df': 1.0, 'max_features': 65536,
              'lowercase': True, 'stop_words': None, 'dtype': np.float64}
    if method == Method.CHARACTER:
        kwargs.update(analyzer='char_wb', ngram_range=(3, 5))
    else:
        kwargs.update(analyzer='word', ngram_range=(1, 1), token_pattern=r'(?u)\b\w\w+\b')
    vectorizer = CountVectorizer(**kwargs)
    counts = vectorizer.fit_transform(texts).tocsr()
    # Count before DF/cap filtering: vocabulary changes must not redefine length.
    analyzer = vectorizer.build_analyzer()
    lengths = np.array([len(analyzer(text)) for text in texts], dtype=np.float64)
    df = np.asarray((counts > 0).sum(axis=0)).ravel()
    transformer = None
    if method != Method.BM25:
        transformer = TfidfTransformer(norm='l2', smooth_idf=True, sublinear_tf=True)
        weights = transformer.fit_transform(counts)
        idf = transformer.idf_
    else:
        average = float(lengths.mean())
        if not average > 0:
            raise ValueError('positive average analyzer length required')
        idf = np.log1p((len(texts) - df + .5) / (df + .5))
        weights = counts.copy()
        rows = np.repeat(np.arange(len(texts)), np.diff(weights.indptr))
        tf = weights.data.copy()
        weights.data = idf[weights.indices] * tf * 2.2 / (
            tf + 1.2 * (.25 + .75 * lengths[rows] / average))
    return Index(method, vectorizer, counts, weights.tocsr(), transformer, lengths, idf)


def evaluate(scores, document_ids, relevant):
    scores = np.asarray(scores, dtype=np.float64)
    if (scores.ndim != 1 or scores.shape != (len(document_ids),) or not np.isfinite(scores).all()
            or (scores < 0).any() or not document_ids or document_ids != sorted(set(document_ids))
            or not relevant or not set(relevant) <= set(document_ids)):
        raise ValueError('aligned complete sorted identities, finite scores and explicit references required')
    # Stable sort preserves the prespecified lexical-ID tie order.
    order = np.argsort(-scores, kind='stable')
    ranks = np.empty(len(order), dtype=np.int64)
    ranks[order] = np.arange(1, len(order) + 1)
    relevant_ranks = {identity: int(ranks[i]) for i, identity in enumerate(document_ids) if identity in relevant}
    first = min(relevant_ranks.values())
    dcg = sum(1 / math.log2(rank + 1) for rank in relevant_ranks.values() if rank <= 10)
    ideal = sum(1 / math.log2(rank + 1) for rank in range(1, min(10, len(relevant)) + 1))
    return {
        'metrics': {'mrr10': 1 / first if first <= 10 else 0,
                    'ndcg10': dcg / ideal,
                    'recall10': sum(r <= 10 for r in relevant_ranks.values()) / len(relevant),
                    'recall100': sum(r <= 100 for r in relevant_ranks.values()) / len(relevant)},
        'first_relevant_rank': first,
        'relevant_ranks': relevant_ranks,
        'positive_score_documents': int(np.count_nonzero(scores)),
        'top100': [{'id': document_ids[i], 'score': float(scores[i])} for i in order[:100]],
    }
