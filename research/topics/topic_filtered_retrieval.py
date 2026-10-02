"""Exact query relevance inside an admitted topic union, with explicit work limits."""
import numpy as np

from topic_routing import Match


def admitted_union(topics, postings, documents, maximum=8192):
    if (type(documents) is not int or not 1 <= documents <= 100000
            or type(maximum) is not int or not 1 <= maximum <= 8192
            or not 1 <= len(postings) <= 8192 or len(topics) > 3
            or len({m.topic for m in topics}) != len(topics)):
        raise ValueError('bounded unique topic selection required')
    union, visits = set(), 0
    for match in topics:
        if (not isinstance(match, Match) or type(match.topic) is not int
                or not 0 <= match.topic < len(postings) or not np.isfinite(match.score)
                or not .55 <= match.score <= 1+1e-12):
            raise ValueError('valid admitted topic required')
        previous = -1
        for doc, confidence in postings[match.topic]:
            if (type(doc) is not int or not previous < doc < documents
                    or not np.isfinite(confidence) or abs(confidence) > 1+1e-12):
                raise ValueError('sorted unique source-bound postings required')
            previous = doc
            union.add(doc)
            visits += 1
            if len(union) > maximum:
                raise ValueError('union work budget exceeded; no truncation or fallback')
    return sorted(union), visits


def score_union(query, vectors, union, budget=1000):
    """Inputs are pinned unit directions; score only the union in chunks of 256."""
    query = np.asarray(query, dtype=np.float64)
    if (vectors.ndim != 2 or query.shape != (vectors.shape[1],)
            or not np.isfinite(query).all() or abs(np.linalg.norm(query)-1) > 1e-12
            or type(budget) is not int or not 1 <= budget <= 1000
            or len(union) > 8192 or union != sorted(set(union))
            or any(type(i) is not int or not 0 <= i < len(vectors) for i in union)):
        raise ValueError('aligned unit query and bounded canonical union required')
    scores = np.empty(len(union), dtype=np.float64)
    for start in range(0, len(union), 256):
        rows = np.asarray(vectors[union[start:start+256]], dtype=np.float64)
        if (not np.isfinite(rows).all()
                or not np.allclose(np.linalg.norm(rows, axis=1), 1, atol=1e-12, rtol=0)):
            raise ValueError('finite admitted unit directions required')
        scores[start:start+len(rows)] = rows @ query
    order = np.argsort(-scores, kind='stable')[:budget]
    return scores, [(union[int(i)], float(scores[i])) for i in order]
