"""Bounded label-only MMR over frozen lexical relevance and phrase directions."""
import numpy as np

from topic_routing import unit_matrix


def select(terms, relevance, vectors, limit=8):
    vectors=unit_matrix(vectors)
    relevance=np.asarray(relevance,dtype=np.float64)
    if (not 1<=len(terms)<=32 or len(set(terms))!=len(terms) or len(vectors)!=len(terms)
            or relevance.shape!=(len(terms),) or not np.isfinite(relevance).all()
            or (relevance<=0).any() or type(limit) is not int or not 1<=limit<=8):
        raise ValueError('bounded source-supported positive candidate pool required')
    relative=relevance/relevance.max();similarity=vectors@vectors.T
    selected=[];steps=[]
    for _ in range(min(limit,len(terms))):
        redundancy=similarity[:,selected].max(axis=1) if selected else np.zeros(len(terms))
        objective=.6*relative-.4*redundancy
        i=min((i for i in range(len(terms)) if i not in selected),key=lambda i:(-float(objective[i]),terms[i]))
        steps.append({'term':terms[i],'score':float(relevance[i]),'relative_relevance':float(relative[i]),
                      'maximum_redundancy':float(redundancy[i]),'objective':float(objective[i])})
        selected.append(i)
    return steps
