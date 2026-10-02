"""Slow independent global-minimum average-linkage audit, not a production fit."""
import math
import numpy as np


def verify(distance,merges):
    current=np.array(distance,dtype=np.float64,copy=True);n=len(current)
    if current.shape!=(n,n) or not np.isfinite(current).all() or not np.array_equal(current,current.T) or len(merges)!=n-1:
        raise ValueError('complete symmetric finite pair distances and merges required')
    np.fill_diagonal(current,np.inf);slots={i:i for i in range(n)};populations=np.ones(n,dtype=np.int64);maximum=0.
    for k,merge in enumerate(merges):
        left,right=int(merge[0]),int(merge[1])
        if left==right or left not in slots or right not in slots:raise ValueError('merge children must be distinct active clusters')
        a,b=slots[left],slots[right];chosen=float(current[a,b]);minimum=float(current.min());gap=chosen-minimum;maximum=max(maximum,gap)
        if gap>1e-12 or not math.isclose(chosen,merge[2],rel_tol=0,abs_tol=1e-12):raise ValueError('captured merge is not a global minimum average distance')
        na,nb=int(populations[a]),int(populations[b])
        if merge[3]!=na+nb:raise ValueError('merge source population differs')
        active=[slot for node,slot in slots.items() if node not in [left,right]]
        values=(na*current[a,active]+nb*current[b,active])/(na+nb)
        current[a,:]=current[:,a]=np.inf;current[b,:]=current[:,b]=np.inf
        current[a,active]=values;current[active,a]=values;populations[a]=na+nb
        del slots[left],slots[right];slots[n+k]=a
    return {'merges':len(merges),'maximum_chosen_minus_global_minimum':maximum}
