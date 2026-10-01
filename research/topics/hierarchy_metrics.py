"""Hierarchy diagnostics; semantic purity requires external labels."""
from itertools import combinations

import numpy as np

from online_tree import Tree


def dendrogram_purity(tree: Tree, labels: dict[str, int]) -> float | None:
    if set(labels) != set(tree.identities):
        raise ValueError("purity labels must cover every leaf exactly")
    total, pairs = 0., 0
    for i, left in enumerate(tree.leaves):
        label = labels[tree.identities[i]]
        for j in range(i + 1, len(tree.leaves)):
            if labels[tree.identities[j]] != label:
                continue
            ancestor = tree.lca(left, tree.leaves[j])
            members = tree.leaf_indices(ancestor)
            total += sum(labels[tree.identities[k]] == label for k in members) / len(members)
            pairs += 1
    return total / pairs if pairs else None


def triplet_signature(tree: Tree, maximum: int = 4096, seed: int = 20261001) -> dict:
    identities = sorted(tree.identities)
    if len(identities) < 3:
        return {"seed": seed, "triplets": [], "closest_pairs": []}
    size = min(maximum, len(identities) * (len(identities) - 1) * (len(identities) - 2) // 6)
    if size < maximum:
        triplets = list(combinations(range(len(identities)), 3))
    else:
        rng, selected = np.random.default_rng(seed), set()
        while len(selected) < size:
            selected.add(tuple(sorted(int(i) for i in rng.choice(len(identities), 3, replace=False))))
        triplets = sorted(selected)
    nodes = dict(zip(tree.identities, tree.leaves))
    pairs = []
    for triplet in triplets:
        candidates = list(combinations(triplet, 2))
        sizes = [tree.lca(nodes[identities[a]], nodes[identities[b]]).count for a, b in candidates]
        pairs.append(candidates[int(np.argmin(sizes))])
    return {"seed": seed, "triplets": triplets, "closest_pairs": pairs}
