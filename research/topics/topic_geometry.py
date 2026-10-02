"""Frozen candidate-bootstrap PCA; no moving coordinates under an online tree."""
from dataclasses import dataclass
import hashlib

import numpy as np
from sklearn.decomposition import PCA


SEED = 'mash-topic-bootstrap-pca25-v1-20261002'


@dataclass(frozen=True)
class Projection:
    mean: np.ndarray
    components: np.ndarray
    bootstrap: tuple[int, ...]
    variance_ratio: np.ndarray

    def transform(self, values):
        values = np.asarray(values, dtype=np.float64)
        if values.ndim != 2 or values.shape[1] != len(self.mean) or not np.isfinite(values).all():
            raise ValueError('finite aligned source coordinates required')
        return (values-self.mean) @ self.components.T


def bootstrap(vectors, ids, dimensions=25, size=1024):
    vectors = np.asarray(vectors, dtype=np.float64)
    if (vectors.ndim != 2 or vectors.shape[0] != len(ids) or ids != sorted(set(ids))
            or type(dimensions) is not int or not 1 <= dimensions <= min(25, vectors.shape[1])
            or type(size) is not int or not dimensions < size <= min(1024,len(ids))
            or not np.isfinite(vectors).all()
            or not np.allclose(np.linalg.norm(vectors,axis=1),1,atol=1e-12,rtol=0)):
        raise ValueError('bounded complete unit-vector bootstrap required')
    chosen = tuple(sorted(range(len(ids)), key=lambda i: hashlib.sha256((SEED+'\0'+ids[i]).encode()).hexdigest())[:size])
    model = PCA(n_components=dimensions, svd_solver='full', whiten=False).fit(vectors[list(chosen)])
    arrays = [model.mean_.copy(),model.components_.copy(),model.explained_variance_ratio_.copy()]
    if not all(np.isfinite(a).all() for a in arrays):
        raise ValueError('nonfinite fitted geometry')
    for a in arrays: a.flags.writeable = False
    return Projection(arrays[0],arrays[1],chosen,arrays[2])


def directions(values):
    values = np.asarray(values, dtype=np.float64)
    if values.ndim != 2 or not values.shape[1] or not np.isfinite(values).all():
        raise ValueError('finite projected coordinates required')
    lengths = np.linalg.norm(values,axis=1)
    valid = lengths > 1e-12
    normalized = np.zeros_like(values)
    np.divide(values,lengths[:,None],out=normalized,where=valid[:,None])
    return normalized, valid, lengths


def diagnostics(original, projected, normalized, sample):
    x=np.asarray(original)[sample];z=np.asarray(projected)[sample];u=np.asarray(normalized)[sample]
    a=x@x.T;b=u@u.T
    mask=np.triu(np.ones(a.shape,dtype=bool),k=1)
    valid=np.linalg.norm(u,axis=1)!=0
    defined=mask & valid[:,None] & valid[None,:]
    delta=np.abs(a[defined]-b[defined])
    x2=np.sum(x*x,axis=1);z2=np.sum(z*z,axis=1)
    original_distance=np.sqrt(np.maximum(x2[:,None]+x2[None,:]-2*a,0))[mask]
    projected_distance=np.sqrt(np.maximum(z2[:,None]+z2[None,:]-2*(z@z.T),0))[mask]
    comparable=original_distance>1e-12
    ratios=projected_distance[comparable]/original_distance[comparable]
    return {'source_pairs':int(mask.sum()),'defined_angle_pairs':len(delta),
            'zero_source_distance_pairs':int((~comparable).sum()),
            'absolute_cosine_change_p50':float(np.median(delta)) if len(delta) else None,
            'absolute_cosine_change_p95':float(np.percentile(delta,95)) if len(delta) else None,
            'absolute_cosine_change_max':float(delta.max()) if len(delta) else None,
            'raw_distance_ratio_p50':float(np.median(ratios)) if len(ratios) else None,
            'raw_distance_ratio_p95':float(np.percentile(ratios,95)) if len(ratios) else None}
