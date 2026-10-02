"""Count pooling follows frozen semantic context boundaries, without new tokens."""
import numpy as np
from scipy import sparse
from semantic_units import Unit,Status,Limit


def pool(counts,units):
    if (not sparse.isspmatrix_csr(counts) or not 1<=len(units)<=2048 or counts.shape[0]!=len(units)
            or not 1<=counts.shape[1]<=20000 or not np.isfinite(counts.data).all()
            or (counts.data<0).any() or not np.equal(counts.data,np.floor(counts.data)).all()):
        raise ValueError('bounded complete nonnegative integer-valued source counts required')
    rows=[];columns=[]
    for end,unit in enumerate(units):
        if (not isinstance(unit,Unit) or type(unit.ordinal) is not int or unit.ordinal!=end
                or not isinstance(unit.status,Status) or not isinstance(unit.limit,Limit)
                or type(unit.tokens) is not int or unit.tokens<0):raise ValueError('complete typed original unit identities required')
        if unit.status!=Status.READY:continue
        if (type(unit.start) is not int or not 0<=unit.start<=end or end-unit.start>4
                or not 2<=unit.tokens<=512 or any(u.status!=Status.READY for u in units[unit.start:end+1])):
            raise ValueError('ready causal context must not cross unsupported gaps or budgets')
        rows.extend([end]*(end-unit.start+1));columns.extend(range(unit.start,end+1))
    incidence=sparse.csr_matrix((np.ones(len(rows),dtype=np.int64),(rows,columns)),shape=(len(units),len(units)))
    result=(incidence@counts).tocsr();result.sum_duplicates();result.sort_indices();result.eliminate_zeros()
    return result
