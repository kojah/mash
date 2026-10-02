"""Post-bootstrap pairs with original full-source LCA denominators."""
from collections import Counter
import math
from captured_hierarchy import traversal,Topology


def topology(stored):
    return Topology(stored['root'],{int(k):tuple(v) for k,v in stored['children'].items()},{int(k):v for k,v in stored['leaves'].items()})


def compact(metrics):
    return {s:{k:metrics[s][k] for k in ['pairs','purity_sum','purity']} for s in ['all','far','crossspan']}


def post_bootstrap(tree,owners,gold,parts,ordinals,minimum_block):
    post,parent,depth=traversal(tree,len(tree.leaves))
    if (type(minimum_block) is not int or minimum_block<0 or not len(owners)==len(gold)==len(parts)==len(ordinals)
            or set(owners)!=set(range(len(tree.leaves))) or sorted(set(ordinals))!=ordinals):raise ValueError('aligned full source and bootstrap bound required')
    leaf_counts={i:Counter() for i in range(len(tree.leaves))}
    for block,topic in zip(owners,gold):
        if topic is not None:leaf_counts[block][topic]+=1
    node_counts={};leaf_nodes={i:n for n,i in tree.leaves.items()}
    for n in post:node_counts[n]=leaf_counts[tree.leaves[n]] if n in tree.leaves else node_counts[tree.children[n][0]]+node_counts[tree.children[n][1]]
    def lca(a,b):
        a,b=leaf_nodes[a],leaf_nodes[b]
        while depth[a]>depth[b]:a=parent[a]
        while depth[b]>depth[a]:b=parent[b]
        while a!=b:a,b=parent[a],parent[b]
        return a
    events={s:Counter() for s in ['all','far','crossspan']}
    for i,topic in enumerate(gold):
        if topic is None or owners[i]<minimum_block:continue
        for j in range(i+1,len(gold)):
            if gold[j]!=topic or owners[j]<minimum_block or owners[i]==owners[j]:continue
            node=lca(owners[i],owners[j]);events['all'][node,topic]+=1
            if ordinals[j]-ordinals[i]>4:
                events['far'][node,topic]+=1
                if parts[i]!=parts[j]:events['crossspan'][node,topic]+=1
    result={}
    for scope,counts in events.items():
        pairs=sum(counts.values());weight=math.fsum(n*node_counts[node][topic]/sum(node_counts[node].values()) for (node,topic),n in counts.items())
        result[scope]={'pairs':pairs,'purity_sum':weight,'purity':weight/pairs if pairs else None}
    return result
