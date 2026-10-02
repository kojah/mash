"""CSR average-link adapter reusing unchanged research rotate/graft operations."""
import numpy as np
from scipy import sparse

from online_tree import Tree,Mode,Linkage


class SparseAverageTree(Tree):
    def __init__(self,mode,maximum_leaves=2000,maximum_dimensions=20000):
        if type(maximum_leaves) is not int or not 1<=maximum_leaves<=2000:raise ValueError('bounded leaf count required')
        if type(maximum_dimensions) is not int or not 1<=maximum_dimensions<=20000:raise ValueError('bounded lexical dimensions required')
        super().__init__(mode,maximum_leaves,linkage=Linkage.AVERAGE)
        self.maximum_dimensions=maximum_dimensions

    def _link_vector(self,total,count):
        return total/count

    def score(self,a,b):
        self.statistics.comparisons+=1
        return float(np.clip(a.link_vector.multiply(b.link_vector).sum(),-1,1))

    def _nearest(self,node,excluded=()):
        self.statistics.nearest_searches+=1
        scores=(self.matrix@node.link_vector.T).toarray().ravel()
        self.statistics.comparisons+=len(scores)-len(excluded)
        scores[list(excluded)]=-np.inf
        best=int(np.argmax(scores)) if len(scores) else None
        return self.leaves[best] if best is not None and np.isfinite(scores[best]) else None

    def insert(self,identity,vector):
        if not isinstance(identity,str) or not identity or identity in self.seen:raise ValueError('unique nonempty leaf identity required')
        if len(self.leaves)>=self.maximum_leaves:raise ValueError('research leaf bound exceeded')
        v=sparse.csr_matrix(vector,dtype=np.float64,copy=True)
        if v.shape[0]!=1 or not 1<=v.shape[1]<=self.maximum_dimensions or not np.isfinite(v.data).all():raise ValueError('finite bounded sparse row required')
        v.sum_duplicates();v.eliminate_zeros();v.sort_indices()
        with np.errstate(over='ignore',under='ignore',invalid='ignore'):
            norm=float(np.sqrt(v.multiply(v).sum()))
        if not np.isfinite(norm) or norm<=0:raise ValueError('positive finite leaf norm required')
        v=v/norm
        if self.matrix is not None and v.shape[1]!=self.matrix.shape[1]:raise ValueError('vector space changed')
        leaf=self._node(v,1,len(self.leaves));nearest=self._nearest(leaf) if self.leaves else None
        if nearest is None:self.root=leaf
        else:
            if self.mode!=Mode.GREEDY:
                while nearest.parent is not None and self.score(leaf,nearest)<self.score(nearest,self.sibling(nearest)):
                    nearest=nearest.parent;self.statistics.rotations+=1
            parent=self._merge(nearest,leaf)
        self.matrix=v.copy() if self.matrix is None else sparse.vstack([self.matrix,v],format='csr')
        self.leaves.append(leaf);self.identities.append(identity);self.seen.add(identity)
        if nearest is not None and self.mode==Mode.GRINCH:
            while parent.parent is not None:parent=self._graft(parent)

    def validate(self):
        if self.root is None:
            if self.nodes or self.leaves:raise ValueError('missing root')
            return
        if self.root.parent is not None:raise ValueError('root has parent')
        visited=set();leaves=set();stack=[(self.root,False)]
        def same(a,b):
            if a.shape!=b.shape:return False
            delta=(a-b).tocoo()
            return all(abs(v)<=1e-12+1e-12*abs(float(b[r,c])) for r,c,v in zip(delta.row,delta.col,delta.data))
        while stack:
            node,expanded=stack.pop()
            if not expanded:
                if node.serial in visited or self.nodes.get(node.serial) is not node:raise ValueError('cycle or detached node')
                visited.add(node.serial);stack.append((node,True))
                if node.children:
                    if node.leaf is not None:raise ValueError('internal leaf identity')
                    for child in node.children:
                        if child.parent is not node:raise ValueError('broken parent')
                        stack.append((child,False))
            elif node.children is None:
                if node.leaf in leaves or node.leaf is None or self.leaves[node.leaf] is not node or node.count!=1:raise ValueError('invalid leaf')
                leaves.add(node.leaf)
                if not same(node.total,self.matrix[node.leaf:node.leaf+1]):raise ValueError('leaf/source direction drift')
            else:
                a,b=node.children
                if node.count!=a.count+b.count or not same(node.total,a.total+b.total):raise ValueError('stale aggregate')
            if not np.isfinite(node.total.data).all() or not same(node.link_vector,node.total/node.count):raise ValueError('stale linkage')
        if visited!=set(self.nodes) or len(leaves)!=len(self.leaves) or len(visited)!=2*len(leaves)-1:raise ValueError('unreachable/nonbinary tree')
