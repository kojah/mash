"""Cut-free, conditional dendrogram purity on immutable captured topologies."""
from collections import Counter
from dataclasses import dataclass
from enum import StrEnum
import math


class Baseline(StrEnum):
    BALANCED = 'temporal_balanced'
    FORWARD = 'temporal_forward_comb'
    REVERSE = 'temporal_reverse_comb'


@dataclass(frozen=True)
class Topology:
    root: int
    children: dict[int, tuple[int, int]]
    leaves: dict[int, int]


def traversal(tree, size):
    if type(size) is not int or not 1 <= size <= 2000:
        raise ValueError('bounded complete source required')
    if set(tree.children) & set(tree.leaves) or sorted(tree.leaves.values()) != list(range(size)):
        raise ValueError('unique complete leaf ownership required')
    parent = {};depth = {};post = [];seen = set();stack = [(tree.root, 0, False)]
    while stack:
        node, level, visited = stack.pop()
        if visited:post.append(node);continue
        if node in seen or node not in tree.children and node not in tree.leaves:
            raise ValueError('acyclic complete topology required')
        seen.add(node);depth[node] = level;stack.append((node, level, True))
        if node in tree.children:
            pair = tree.children[node]
            if len(pair) != 2 or pair[0] == pair[1]:raise ValueError('binary distinct children required')
            for child in reversed(pair):
                if child in parent:raise ValueError('unique parent required')
                parent[child] = node;stack.append((child, level+1, False))
    if seen != set(tree.children) | set(tree.leaves) or tree.root in parent:
        raise ValueError('connected root required')
    return post, parent, depth


def captured(record):
    state = record['state'];arrival = state['arrival'];size = len(record['source_ordinals'])
    if sorted(arrival) != list(range(size)):raise ValueError('complete original arrival required')
    children = {};leaves = {};counts = {};parents = {}
    for n in state['nodes']:
        serial = n['serial']
        if serial in counts:raise ValueError('duplicate node identity')
        counts[serial] = n['count'];parents[serial] = n['parent']
        if n['children'] is None:
            if type(n['leaf']) is not int or not 0 <= n['leaf'] < size:raise ValueError('source leaf required')
            leaves[serial] = arrival[n['leaf']]
        else:
            if n['leaf'] is not None:raise ValueError('internal leaf identity forbidden')
            children[serial] = tuple(n['children'])
    tree = Topology(state['root'], children, leaves);post, parent, _ = traversal(tree, size)
    actual = {}
    for node in post:
        actual[node] = sum(actual[c] for c in children[node]) if node in children else 1
        if counts[node] != actual[node] or parents[node] != parent.get(node):raise ValueError('captured count/parent drift')
    return tree


def temporal(size, method):
    method = Baseline(method);children = {};leaves = {i:i for i in range(size)};serial = size
    def join(a, b):
        nonlocal serial
        node = serial;serial += 1;children[node] = (a,b);return node
    if method == Baseline.BALANCED:
        def build(lo, hi):
            if hi-lo == 1:return lo
            mid = (lo+hi)//2;return join(build(lo,mid),build(mid,hi))
        root = build(0,size)
    else:
        order = list(range(size))
        if method == Baseline.REVERSE:order.reverse()
        root = order[0]
        for leaf in order[1:]:root = join(root,leaf)
    tree = Topology(root,children,leaves);traversal(tree,size);return tree


def evaluate(tree, gold, components, ordinals, topic_count):
    """Every positive pair contributes its class fraction at its unique LCA.

    Unknown/multi-annotated leaves remain in geometry, outside the conditional
    purity denominator. Far and crossspan positives exclude original gaps <=4.
    Component IDs are native merged span identities, never learned labels.
    """
    size = len(gold);post,parent,depth = traversal(tree,size)
    if (len(components) != size or len(ordinals) != size or sorted(set(ordinals)) != ordinals
            or any(type(i) is not int or i < 0 for i in ordinals)
            or any(g is not None and (type(g) is not int or not 0 <= g < topic_count) for g in gold)
            or any((g is None) != (c is None) for g,c in zip(gold,components))):
        raise ValueError('aligned original singleton-known evidence required')
    leaf_nodes = {i:n for n,i in tree.leaves.items()}
    def lca(a,b):
        while depth[a] > depth[b]:a = parent[a]
        while depth[b] > depth[a]:b = parent[b]
        while a != b:a,b = parent[a],parent[b]
        return a
    near = {};near_return = {}
    for i,g in enumerate(gold):
        if g is None:continue
        for j in range(i+1,size):
            if ordinals[j]-ordinals[i] > 4:break
            if gold[j] != g:continue
            node = lca(leaf_nodes[i],leaf_nodes[j]);near.setdefault(node,Counter())[g] += 1
            if components[i] != components[j]:near_return.setdefault(node,Counter())[g] += 1
    counts = {};spans = {};events = {k:[[] for _ in range(topic_count)] for k in ['all','far','crossspan']}
    for node in post:
        if node in tree.leaves:
            i = tree.leaves[node];g = gold[i]
            counts[node] = Counter() if g is None else Counter({g:1})
            spans[node] = Counter() if g is None else Counter({(g,components[i]):1})
            continue
        a,b = tree.children[node];counts[node] = counts[a]+counts[b];spans[node] = spans[a]+spans[b]
        known = sum(counts[node].values())
        same_span = Counter()
        for (g,c),n in spans[a].items():same_span[g] += n*spans[b][g,c]
        for g,n in counts[a].items():
            pairs = n*counts[b][g]
            values = {'all':pairs,'far':pairs-near.get(node,{}).get(g,0),
                      'crossspan':pairs-same_span[g]-near_return.get(node,{}).get(g,0)}
            for scope,p in values.items():
                if p < 0:raise ValueError('negative LCA pair accounting')
                if p:events[scope][g].append((p,p*counts[node][g]/known,p*known,p if node == tree.root else 0))
    totals = counts[tree.root];known = sum(totals.values());result = {}
    for scope,topics in events.items():
        support = []
        for g,items in enumerate(topics):
            pairs = sum(r[0] for r in items);weight = math.fsum(r[1] for r in items)
            support.append({'topic':g,'known_rows':totals[g],'pairs':pairs,'purity_sum':weight,
                            'lca_known_rows_sum':sum(r[2] for r in items),'root_lca_pairs':sum(r[3] for r in items),
                            'purity':weight/pairs if pairs else None})
        pairs = sum(r['pairs'] for r in support);weight = math.fsum(r['purity_sum'] for r in support)
        root_weight = math.fsum(r['pairs']*totals[r['topic']]/known for r in support) if known else 0.
        result[scope] = {'pairs':pairs,'purity_sum':weight,'purity':weight/pairs if pairs else None,
                         'root_only_purity_sum':root_weight,'root_only_purity':root_weight/pairs if pairs else None,
                         'lca_known_rows_sum':sum(r['lca_known_rows_sum'] for r in support),
                         'root_lca_pairs':sum(r['root_lca_pairs'] for r in support),'topics':support}
    return result
