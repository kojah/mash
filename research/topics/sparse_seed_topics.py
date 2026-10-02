"""Insert-only fixed-source seed control, deliberately without semantic hierarchy."""
from collections import Counter
import time

import numpy as np
from scipy import sparse


def discover(matrix, *, reverse=False):
    if (not sparse.isspmatrix_csr(matrix) or not 0 < matrix.shape[0] <= 2048
            or not 0 < matrix.shape[1] <= 20000 or type(reverse) is not bool
            or not np.isfinite(matrix.data).all()):raise ValueError('bounded finite CSR source vectors required')
    norms=np.asarray(matrix.multiply(matrix).sum(axis=1)).ravel()
    if not np.allclose(norms,1,rtol=0,atol=1e-12):raise ValueError('nonempty unit source vectors required')
    representatives=sparse.csr_matrix((0,matrix.shape[1]),dtype=np.float64)
    seeds=[];assigned=np.full(matrix.shape[0],-1,dtype=np.int64);events=[];latencies=[];comparisons=0
    for i in (range(matrix.shape[0]-1,-1,-1) if reverse else range(matrix.shape[0])):
        start=time.perf_counter();nearest=None;score=None
        if seeds:
            values=(representatives @ matrix[i].T).toarray().ravel();comparisons+=len(seeds)
            nearest=int(np.argmax(values));score=float(values[nearest])
        created=score is None or score < .55
        if created:
            if len(seeds)==2048:raise ValueError('seed budget exhausted; no forced assignment')
            target=len(seeds);seeds.append(i);representatives=sparse.vstack([representatives,matrix[i]],format='csr')
        else:target=nearest
        assigned[i]=target;events.append({'source_row':i,'nearest':nearest,'best_score':score,'assigned':target,'created':created})
        latencies.append((time.perf_counter()-start)*1000)
    return {'assignments':assigned.tolist(),'seeds':seeds,'events':events,'comparisons':comparisons,'latencies_ms':latencies}


def pair_counts(assignments, gold):
    if len(assignments)!=len(gold) or any(type(x) is not int or x<0 for x in assignments):raise ValueError('aligned complete assignments required')
    selected=[(p,g) for p,g in zip(assignments,gold) if g is not None]
    if any(type(g) is not int or g<0 for _,g in selected):raise ValueError('nonnegative singleton gold or unknown required')
    choose=lambda n:n*(n-1)//2
    predicted=sum(choose(n) for n in Counter(p for p,g in selected).values())
    relevant=sum(choose(n) for n in Counter(g for p,g in selected).values())
    tp=sum(choose(n) for n in Counter(selected).values());total=choose(len(selected))
    return {'known_rows':len(selected),'pairs':total,'tp':tp,'fp':predicted-tp,'fn':relevant-tp,'tn':total-predicted-relevant+tp}


def labels(counts, assignments, idf, terms):
    if (not sparse.isspmatrix_csr(counts) or counts.shape[0]!=len(assignments)
            or len(terms)!=counts.shape[1] or terms!=sorted(set(terms))
            or np.asarray(idf).shape!=(len(terms),) or not np.isfinite(idf).all() or (np.asarray(idf)<=0).any()
            or any(type(x) is not int or x<0 for x in assignments)):
        raise ValueError('aligned original counts, assignments and frozen IDF required')
    groups=max(assignments)+1
    if set(assignments)!=set(range(groups)):raise ValueError('contiguous complete seed ownership required')
    incidence=sparse.csr_matrix((np.ones(len(assignments)),(assignments,np.arange(len(assignments)))),shape=(groups,len(assignments)))
    combined=(incidence @ counts).tocsr();lengths=np.asarray(combined.sum(axis=1)).ravel()
    frequency=np.asarray(combined.sum(axis=0)).ravel();result=[]
    for i in range(groups):
        row=combined.getrow(i)
        scores=row.data/max(lengths[i],1)*np.log1p(lengths.mean()/frequency[row.indices])*np.asarray(idf)[row.indices]
        ordered=sorted(zip(row.indices,scores,row.data),key=lambda r:(-r[1],terms[r[0]]))[:8]
        result.append([{'term':terms[j],'score':float(score),'source_count':int(n)} for j,score,n in ordered])
    return result
