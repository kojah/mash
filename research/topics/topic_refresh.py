"""Deferred lexical hierarchy/labels and multi-topic thread tags; no inference."""
import hashlib
import json
import numpy as np
from scipy import sparse
from episode_blocks import lexical
from block_hierarchy import average
from topic_features import terms,assignments,lexical_counts,labels_for_sets


def refresh(capture,threshold=.55,relative=.8,maximum_tags=3):
    if not 0 < threshold <= 1 or not 0 < relative <= 1 or type(maximum_tags) is not int or not 1 <= maximum_tags <= 3:
        raise ValueError('explicit bounded membership parameters required')
    records = capture.records; ids = capture.leaf_ids
    ready,memberships,leaf_sources = assignments(capture,threshold,relative,maximum_tags)
    vocabulary,counts,df,idf = lexical_counts(records,ready)
    pooled = sparse.vstack([sparse.csr_matrix(counts[sorted(s)].sum(axis=0)) for s in leaf_sources],format='csr') if ids else sparse.csr_matrix((0,len(vocabulary)))
    topology = None; merges = []; children = {}; descendants = {i:[i] for i in range(len(ids))}; node_sources = {i:set(s) for i,s in enumerate(leaf_sources)}
    if ids:
        features = lexical(pooled,idf).tocsr(); topology,merges = average((features@features.T).toarray())
        children = topology.children
        for node,(left,right) in sorted(children.items()):
            descendants[node] = sorted(descendants[left]+descendants[right]); node_sources[node] = node_sources[left]|node_sources[right]
    identities = {node:('leaf-'+str(ids[node]) if node < len(ids) else 'branch-'+hashlib.sha256(json.dumps([ids[i] for i in descendants[node]],separators=(',',':')).encode()).hexdigest()[:24]) for node in node_sources}
    parents = {child:node for node,pair in children.items() for child in pair}; nodes = []
    label_rows = labels_for_sets(counts,vocabulary,ready,df,idf,[node_sources[i] for i in sorted(node_sources)])
    for position,node in enumerate(sorted(node_sources)):
        members = sorted(node_sources[node]); keywords = label_rows[position]
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
