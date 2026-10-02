"""Bounded character feature control; padded fragments are not topic labels."""
import numpy as np
from sklearn.feature_extraction.text import CountVectorizer


def count_channels(full, title, prose, code):
    fields = (full, title, prose, code)
    if (not 1 <= len(full) <= 512 or any(len(f) != len(full) for f in fields)
            or any(not isinstance(t, str) or len(t.encode()) > 65536 for f in fields for t in f)):
        raise ValueError('aligned bounded source text required')
    vectorizer = CountVectorizer(analyzer='char_wb', ngram_range=(3, 5), lowercase=True,
                                 min_df=2, max_df=.40, max_features=8192)
    whole = vectorizer.fit_transform(full).toarray()
    matrices = [vectorizer.transform(field).toarray() for field in (title, prose, code)]
    if not np.array_equal(whole, sum(matrices)):
        raise ValueError('character source-channel count conservation failed')
    return vectorizer.get_feature_names_out().tolist(), matrices


def idf_vectors(mass, counts):
    mass, counts = [np.asarray(x, dtype=np.float64) for x in (mass, counts)]
    if (mass.ndim != 2 or not mass.shape[0] or not mass.shape[1] or mass.shape != counts.shape
            or not np.isfinite(mass).all() or not np.isfinite(counts).all()
            or (mass < 0).any() or (counts < 0).any()):
        raise ValueError('aligned finite nonnegative counts/mass required')
    df = (counts > 0).sum(0)
    idf = np.log((1 + len(counts)) / (1 + df)) + 1
    vectors = mass * idf
    lengths = np.linalg.norm(vectors, axis=1, keepdims=True)
    vectors = np.divide(vectors, lengths, out=np.zeros_like(vectors), where=lengths > 0)
    return vectors, df, idf


def sparse_rows(matrix):
    return [{'indices': np.flatnonzero(row).tolist(), 'values': row[row != 0].tolist()} for row in matrix]
