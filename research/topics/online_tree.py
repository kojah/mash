"""Exact-neighbor cosine-linkage GRINCH-style research control, not production."""
from dataclasses import dataclass
from enum import StrEnum

import numpy as np

from data import normalized


class Mode(StrEnum):
    GREEDY = "greedy"
    ROTATE = "rotate"
    GRINCH = "grinch"


class Linkage(StrEnum):
    CENTROID = "centroid"
    AVERAGE = "average"


@dataclass(eq=False)
class Node:
    serial: int
    total: np.ndarray
    link_vector: np.ndarray
    count: int
    leaf: int | None = None
    parent: "Node | None" = None
    children: tuple["Node", "Node"] | None = None


@dataclass
class Statistics:
    comparisons: int = 0
    nearest_searches: int = 0
    rotations: int = 0
    graft_checks: int = 0
    grafts: int = 0
    restructure_swaps: int = 0


class Tree:
    """Insert-only prototype; float64, stable arrival-order ties, no ANN/caps."""
    def __init__(self, mode: Mode, maximum_leaves: int = 2000, maximum_dimensions: int = 1024,
                 linkage: Linkage = Linkage.CENTROID):
        if maximum_leaves < 1:
            raise ValueError("positive research leaf bound required")
        if type(maximum_dimensions) is not int or not 1 <= maximum_dimensions <= 8192:
            raise ValueError("bounded positive research dimension limit required")
        self.mode = Mode(mode)
        self.linkage = Linkage(linkage)
        self.maximum_leaves = maximum_leaves
        self.maximum_dimensions = maximum_dimensions
        self.root: Node | None = None
        self.nodes: dict[int, Node] = {}
        self.leaves: list[Node] = []
        self.identities: list[str] = []
        self.seen: set[str] = set()
        self.matrix: np.ndarray | None = None
        self.statistics = Statistics()
        self.serial = 0

    def _link_vector(self, total: np.ndarray, count: int) -> np.ndarray:
        if self.linkage == Linkage.AVERAGE:
            return total / count
        norm = np.linalg.norm(total)
        return total / norm if norm > np.finfo(np.float64).eps else np.zeros_like(total)

    def _node(self, total: np.ndarray, count: int, leaf: int | None = None) -> Node:
        node = Node(self.serial, total, self._link_vector(total, count), count, leaf)
        self.nodes[node.serial] = node
        self.serial += 1
        return node

    def _refresh(self, node: Node | None) -> None:
        while node is not None:
            if node.children is not None:
                left, right = node.children
                node.total = left.total + right.total
                node.count = left.count + right.count
                node.link_vector = self._link_vector(node.total, node.count)
            node = node.parent

    def score(self, a: Node, b: Node) -> float:
        self.statistics.comparisons += 1
        return float(np.clip(a.link_vector @ b.link_vector, -1, 1))

    @staticmethod
    def sibling(node: Node) -> Node:
        if node.parent is None or node.parent.children is None:
            raise ValueError("root has no sibling")
        left, right = node.parent.children
        if node is not left and node is not right:
            raise ValueError("broken parent link")
        return right if node is left else left

    @staticmethod
    def leaf_indices(node: Node) -> tuple[int, ...]:
        stack, result = [node], []
        while stack:
            current = stack.pop()
            if current.children is None:
                result.append(current.leaf)
            else:
                stack.extend(current.children)
        return tuple(sorted(result))

    @staticmethod
    def ancestors(node: Node) -> tuple[Node, ...]:
        result = []
        while node is not None:
            result.append(node)
            node = node.parent
        return tuple(result)

    def lca(self, a: Node, b: Node) -> Node:
        ancestors = {n.serial for n in self.ancestors(a)}
        for node in self.ancestors(b):
            if node.serial in ancestors:
                return node
        raise ValueError("disconnected nodes")

    def _nearest(self, node: Node, excluded: tuple[int, ...] = ()) -> Node | None:
        self.statistics.nearest_searches += 1
        scores = self.matrix[:len(self.leaves)] @ node.link_vector
        self.statistics.comparisons += len(scores) - len(excluded)
        scores[list(excluded)] = -np.inf
        best = int(np.argmax(scores)) if len(scores) else None
        return self.leaves[best] if best is not None and np.isfinite(scores[best]) else None

    def _merge(self, a: Node, b: Node) -> Node:
        """Detach b, splice its sibling up, then make b and a siblings."""
        old_parent = b.parent
        old_grandparent = old_parent.parent if old_parent else None
        if old_parent is not None:
            remaining = self.sibling(b)
            remaining.parent = old_grandparent
            if old_grandparent is None:
                self.root = remaining
            else:
                old_grandparent.children = tuple(remaining if n is old_parent else n for n in old_grandparent.children)
            del self.nodes[old_parent.serial]
            old_parent.children, old_parent.parent = None, None
            b.parent = None
            self._refresh(old_grandparent)
        parent = self._node(a.total + b.total, a.count + b.count)
        parent.parent = a.parent
        if a.parent is None:
            self.root = parent
        else:
            a.parent.children = tuple(parent if n is a else n for n in a.parent.children)
        parent.children = (a, b)
        a.parent = b.parent = parent
        self._refresh(parent)
        return parent

    def _swap(self, a: Node, b: Node) -> None:
        if a is b or a.parent is b.parent:
            return
        if a.parent is None or b.parent is None or self.lca(a, b) in (a, b):
            raise ValueError("swap requires disjoint nonroot subtrees")
        pa, pb = a.parent, b.parent
        pa.children = tuple(b if n is a else n for n in pa.children)
        pb.children = tuple(a if n is b else n for n in pb.children)
        a.parent, b.parent = pb, pa
        self._refresh(pa)
        self._refresh(pb)

    def _restructure(self, node: Node, stop: Node) -> None:
        while node is not stop:
            candidates = []
            for ancestor in self.ancestors(node)[1:]:
                if ancestor is stop:
                    break
                candidates.append(self.sibling(ancestor))
            if candidates:
                scored = [(self.score(node, candidate), candidate.serial, candidate) for candidate in candidates]
                value, _, best = max(scored, key=lambda x: (x[0], -x[1]))
                if value > self.score(node, self.sibling(node)):
                    self._swap(self.sibling(node), best)
                    self.statistics.restructure_swaps += 1
            node = node.parent
            if node is None:
                raise ValueError("restructuring escaped its ancestor")

    def _graft(self, start: Node) -> Node:
        other = self._nearest(start, self.leaf_indices(start))
        if other is None:
            return self.root
        stop, node = self.lca(start, other), start
        while node is not stop and other is not stop and self.sibling(node) is not other:
            self.statistics.graft_checks += 1
            value = self.score(node, other)
            own = self.score(node, self.sibling(node))
            theirs = self.score(other, self.sibling(other))
            if value > max(own, theirs):
                displaced = self.sibling(node)
                parent = self._merge(node, other)
                self.statistics.grafts += 1
                if displaced.serial in self.nodes:
                    self._restructure(displaced, self.lca(displaced, parent))
                return parent
            # Strict mutual preference; ties ascend rather than stall.
            if value <= theirs:
                other = other.parent
            else:
                node = node.parent
        return stop

    def insert(self, identity: str, vector: np.ndarray) -> None:
        if not isinstance(identity, str) or not identity or identity in self.seen:
            raise ValueError("unique nonempty leaf identity required")
        if len(self.leaves) >= self.maximum_leaves:
            raise ValueError("research leaf bound exceeded")
        vector = np.asarray(vector, dtype=np.float64)
        if vector.ndim != 1 or not 1 <= len(vector) <= self.maximum_dimensions:
            raise ValueError("invalid vector dimensions")
        vector = normalized(vector).copy()
        if self.matrix is None:
            self.matrix = np.empty((self.maximum_leaves, len(vector)), dtype=np.float64)
        elif len(vector) != self.matrix.shape[1]:
            raise ValueError("vector space changed")
        leaf = self._node(vector, 1, len(self.leaves))
        nearest = self._nearest(leaf) if self.leaves else None
        if nearest is None:
            self.root = leaf
        else:
            if self.mode != Mode.GREEDY:
                while nearest.parent is not None and self.score(leaf, nearest) < self.score(nearest, self.sibling(nearest)):
                    nearest = nearest.parent
                    self.statistics.rotations += 1
            parent = self._merge(nearest, leaf)
        self.matrix[len(self.leaves)] = vector
        self.leaves.append(leaf)
        self.identities.append(identity)
        self.seen.add(identity)
        if nearest is not None and self.mode == Mode.GRINCH:
            while parent.parent is not None:
                # The returned merger/LCA is the next candidate, not its parent:
                # skipping it can strand a distant leaf of the same chain.
                parent = self._graft(parent)

    def validate(self) -> None:
        """Check the entire live tree; excluded from insertion timing."""
        if self.root is None:
            if self.nodes or self.leaves:
                raise ValueError("missing root")
            return
        if self.root.parent is not None:
            raise ValueError("root has parent")
        visited, leaves, stack = set(), set(), [(self.root, False)]
        while stack:
            node, expanded = stack.pop()
            if not expanded:
                if node.serial in visited or self.nodes.get(node.serial) is not node:
                    raise ValueError("cycle, repeated child or detached live node")
                visited.add(node.serial)
                stack.append((node, True))
                if node.children:
                    if node.leaf is not None:
                        raise ValueError("internal node has leaf identity")
                    for child in node.children:
                        if child.parent is not node:
                            raise ValueError("broken parent link")
                        stack.append((child, False))
            elif node.children is None:
                if node.leaf in leaves or node.leaf is None or self.leaves[node.leaf] is not node or node.count != 1:
                    raise ValueError("invalid leaf")
                leaves.add(node.leaf)
            else:
                left, right = node.children
                total = left.total + right.total
                if node.count != left.count + right.count or not np.allclose(node.total, total, rtol=1e-12, atol=1e-12):
                    raise ValueError("stale subtree aggregate")
            expected = self._link_vector(node.total, node.count)
            if not np.isfinite(node.total).all() or not np.allclose(node.link_vector, expected, rtol=1e-12, atol=1e-12):
                raise ValueError("stale subtree linkage vector")
        if visited != set(self.nodes) or len(leaves) != len(self.leaves) or len(visited) != 2 * len(leaves) - 1:
            raise ValueError("unreachable or nonbinary live tree")

    def cut(self, cosine: float) -> tuple[tuple[int, ...], ...]:
        """A clade qualifies only if every internal merger meets the fixed cutoff."""
        if not -1 <= cosine <= 1 or self.root is None:
            raise ValueError("nonempty tree and valid cut required")
        minimum = {}
        stack = [(self.root, False)]
        while stack:
            node, expanded = stack.pop()
            if node.children is None:
                minimum[node.serial] = 1.
            elif not expanded:
                stack.append((node, True))
                stack.extend((n, False) for n in node.children)
            else:
                left, right = node.children
                minimum[node.serial] = min(self.score(left, right), minimum[left.serial], minimum[right.serial])
        result, stack = [], [self.root]
        while stack:
            node = stack.pop()
            if minimum[node.serial] >= cosine:
                result.append(self.leaf_indices(node))
            else:
                stack.extend(node.children)
        return tuple(sorted(result))
