"""Shared source assignment and lexical labeling for research refresh paths."""
import re
import numpy as np
from scipy import sparse
from sklearn.feature_extraction.text import CountVectorizer
from label_noise import EXCLUDED
from subject_labels import ranked_terms


TOKEN = re.compile(r'(?u)\b\w[\w.+:/-]*\b')


def terms(text):
    previous = None; end = 0
    for match in TOKEN.finditer(text):
        word = match.group().lower()
        if word in EXCLUDED or len(word) > 64: previous = None
        else:
            yield word
            if previous is not None and text[end:match.start()].isspace(): yield previous+' '+word
            previous = word
        end = match.end()


def assignments(capture,threshold,relative,maximum_tags):
    records = capture.records; ids = capture.leaf_ids; memberships = {}; leaf_sources = [set() for _ in ids]
    ready = [i for i,r in enumerate(records) if r.source.vector is not None]
    scores = np.stack([records[i].source.vector for i in ready])@capture.centers.T if ready else np.empty((0,len(ids)))
    positions_by_leaf = {identity:i for i,identity in enumerate(ids)}
    for position,i in enumerate(ready):
        row = scores[position]; own = positions_by_leaf.get(records[i].anchor)
        # Exclude a point's own contribution when judging a supported centroid.
        # Singleton self-identity remains explicitly provisional/weak, not gold.
        if own is not None and capture.counts[own] > 1:
            other = capture.sums[own]-records[i].source.vector; norm = float(np.linalg.norm(other))
            row[own] = float((other/norm)@records[i].source.vector) if norm > 1e-12 else 0.
        best = float(row.max()) if len(row) else 0
        order = sorted((j for j,v in enumerate(row) if v >= threshold and v >= best*relative),key=lambda j:(-row[j],ids[j]))[:maximum_tags]
        memberships[records[i].source.id] = [{'topic':'leaf-'+str(ids[j]),'score':float(row[j])} for j in order]
        for j in order: leaf_sources[j].add(i)
    for r in records: memberships.setdefault(r.source.id,[])
    return ready,memberships,leaf_sources


def lexical_counts(records,ready):
    vocabulary = []; counts = sparse.csr_matrix((len(records),0))
    if ready and any(next(terms(records[i].source.text),None) is not None for i in ready):
        vectorizer = CountVectorizer(analyzer=terms,max_features=8192,dtype=np.float64)
        matrix = vectorizer.fit_transform(records[i].source.text for i in ready); vocabulary = vectorizer.get_feature_names_out().tolist()
        counts = sparse.csr_matrix((len(records),len(vocabulary)),dtype=float).tolil(); counts[ready,:] = matrix; counts = counts.tocsr()
    df = np.asarray((counts[ready] > 0).sum(axis=0)).ravel() if ready else np.zeros(len(vocabulary))
    idf = np.log((1+len(ready))/(1+df))+1
    return vocabulary,counts,df,idf


def labels_for_sets(counts,vocabulary,ready,df,idf,source_sets):
    classes = sparse.vstack([sparse.csr_matrix(counts[sorted(source_sets[i])].sum(axis=0)) for i in range(len(source_sets))],format='csr') if source_sets else sparse.csr_matrix((0,len(vocabulary)))
    lengths = np.asarray(classes.sum(axis=1)).ravel(); frequency = np.asarray(classes.sum(axis=0)).ravel()
    weights = classes.copy(); row_ids = np.repeat(np.arange(len(lengths)),np.diff(weights.indptr))
    if weights.nnz:
        weights.data = weights.data/np.maximum(lengths[row_ids],1)*np.log1p(lengths.mean()/np.maximum(frequency[weights.indices],1))*idf[weights.indices]
    positions = {term:i for i,term in enumerate(vocabulary)}
    global_allowed = np.array([df[j] <= .4*len(ready) and any(c.isalpha() for c in term) for j,term in enumerate(vocabulary)],dtype=bool)
    output = []
    for position,node in enumerate(range(len(source_sets))):
        members = sorted(source_sets[node]); member_df = np.asarray((counts[members] > 0).sum(axis=0)).ravel()
        allowed = (member_df >= 2)&global_allowed
        values = weights[position].toarray().ravel()*allowed
        keywords = ranked_terms(values,vocabulary,diverse=True,limit=8) if vocabulary else []
        for label in keywords:
            j = positions[label['term']]; label['source_support'] = int(member_df[j])
        output.append(keywords)
    return output
