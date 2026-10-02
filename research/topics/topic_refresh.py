"""Deferred lexical hierarchy/labels and multi-topic thread tags; no inference."""
import hashlib
import json
import re
import numpy as np
from scipy import sparse
from sklearn.feature_extraction.text import CountVectorizer
from label_noise import EXCLUDED
from episode_blocks import lexical
from block_hierarchy import average
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


def refresh(capture,threshold=.55,relative=.8,maximum_tags=3):
    if not 0 < threshold <= 1 or not 0 < relative <= 1 or type(maximum_tags) is not int or not 1 <= maximum_tags <= 3:
        raise ValueError('explicit bounded membership parameters required')
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
    vocabulary = []; counts = sparse.csr_matrix((len(records),0))
    if ready and any(next(terms(records[i].source.text),None) is not None for i in ready):
        vectorizer = CountVectorizer(analyzer=terms,max_features=8192,dtype=np.float64)
        matrix = vectorizer.fit_transform(records[i].source.text for i in ready); vocabulary = vectorizer.get_feature_names_out().tolist()
        counts = sparse.csr_matrix((len(records),len(vocabulary)),dtype=float).tolil(); counts[ready,:] = matrix; counts = counts.tocsr()
    df = np.asarray((counts[ready] > 0).sum(axis=0)).ravel() if ready else np.zeros(len(vocabulary))
    idf = np.log((1+len(ready))/(1+df))+1
    pooled = sparse.vstack([sparse.csr_matrix(counts[sorted(s)].sum(axis=0)) for s in leaf_sources],format='csr') if ids else sparse.csr_matrix((0,len(vocabulary)))
    topology = None; merges = []; children = {}; descendants = {i:[i] for i in range(len(ids))}; node_sources = {i:set(s) for i,s in enumerate(leaf_sources)}
    if ids:
        features = lexical(pooled,idf).tocsr(); topology,merges = average((features@features.T).toarray())
        children = topology.children
        for node,(left,right) in sorted(children.items()):
            descendants[node] = sorted(descendants[left]+descendants[right]); node_sources[node] = node_sources[left]|node_sources[right]
    identities = {node:('leaf-'+str(ids[node]) if node < len(ids) else 'branch-'+hashlib.sha256(json.dumps([ids[i] for i in descendants[node]],separators=(',',':')).encode()).hexdigest()[:24]) for node in node_sources}
    parents = {child:node for node,pair in children.items() for child in pair}; nodes = []
    classes = sparse.vstack([sparse.csr_matrix(counts[sorted(node_sources[i])].sum(axis=0)) for i in sorted(node_sources)],format='csr') if node_sources else sparse.csr_matrix((0,len(vocabulary)))
    lengths = np.asarray(classes.sum(axis=1)).ravel(); frequency = np.asarray(classes.sum(axis=0)).ravel()
    weights = classes.copy(); row_ids = np.repeat(np.arange(len(lengths)),np.diff(weights.indptr))
    if weights.nnz:
        weights.data = weights.data/np.maximum(lengths[row_ids],1)*np.log1p(lengths.mean()/np.maximum(frequency[weights.indices],1))*idf[weights.indices]
    positions = {term:i for i,term in enumerate(vocabulary)}
    for position,node in enumerate(sorted(node_sources)):
        members = sorted(node_sources[node]); member_df = np.asarray((counts[members] > 0).sum(axis=0)).ravel()
        allowed = np.array([member_df[j] >= 2 and df[j] <= .4*len(ready) and any(c.isalpha() for c in term) for j,term in enumerate(vocabulary)])
        values = weights[position].toarray().ravel()*allowed
        keywords = ranked_terms(values,vocabulary,diverse=True,limit=8) if vocabulary else []
        for label in keywords:
            j = positions[label['term']]; label['source_support'] = int(member_df[j])
        nodes.append({'id':identities[node],'children':[identities[i] for i in children.get(node,())],
            'parent':identities[parents[node]] if node in parents else None,'kind':'leaf' if node < len(ids) else 'branch',
            'labels':keywords,'label_support':'supported' if len(keywords) >= 2 else 'weak',
            'sources':[records[i].source.id for i in members],'threads':sorted({records[i].source.thread for i in members})})
    node_map = {n['id']:n for n in nodes}; threads = {}
    for r in records:
        thread = threads.setdefault(r.source.thread,{'thread':r.source.thread,'provider':r.source.provider,'sources':[],'direct_tags':set(),'ancestor_tags':set()})
        if thread['provider'] != r.source.provider: raise ValueError('thread provider scope differs')
        thread['sources'].append(r.source.id)
        for match in memberships[r.source.id]:
            leaf = match['topic']; thread['direct_tags'].add(leaf); parent = node_map[leaf]['parent']
            while parent is not None: thread['ancestor_tags'].add(parent); parent = node_map[parent]['parent']
    for row in threads.values():
        row['direct_tags'] = sorted(row['direct_tags']); row['ancestor_tags'] = sorted(row['ancestor_tags']); row['sources'].sort()
    return {'schema_version':1,'revision':capture.revision,'versions':{r.source.id:r.version for r in records},
        'root':identities[topology.root] if topology is not None else None,'nodes':nodes,'memberships':memberships,
        'threads':[threads[i] for i in sorted(threads)],'geometry':'source-pooled lexical average over cosine-routing leaves',
        'parameters':{'threshold':threshold,'relative':relative,'maximum_tags':maximum_tags,'maximum_vocabulary':8192,'minimum_label_source_support':2,'maximum_label_df_fraction':.4,'own_centroid':'leave_one_out_when_nonsingleton'},
        'diagnostics':{'sources':len(records),'stored_vectors':len(ready),'missing_vectors':len(records)-len(ready),
            'untagged_ready_sources':sum(not memberships[records[i].source.id] for i in ready),'leaves':len(ids),
            'multi_tag_sources':sum(len(v)>1 for v in memberships.values()),'weak_nodes':sum(n['label_support']=='weak' for n in nodes),
            'vocabulary':len(vocabulary),'zero_lexical_leaves':int((pooled.getnnz(axis=1)==0).sum())}}
