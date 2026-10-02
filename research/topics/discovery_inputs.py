"""Read only pinned full-source causal evidence; never infer or download models."""
from dataclasses import dataclass
import gc
import hashlib
import json
from pathlib import Path

import numpy as np
from scipy import sparse

from qmsum import load
from qmsum_semantic_units import VECTORS
from qmsum_semantic_discovery import LABELS


UNITS='e22d6878b6640dec38f7f2ced42956ae73abfaa3e1502741743a82d44c3eff33'


def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()


@dataclass(frozen=True)
class Inputs:
    capture: str
    source: str
    meetings: tuple
    raw: np.ndarray
    ready: np.ndarray
    tokens: np.ndarray
    counts: sparse.csr_matrix
    idf: np.ndarray
    terms: list[str]
    lineage: list[dict]
    seed_summary: dict
    reference: dict


def read(a):
    if digest(a.vectors/'results.json')!=VECTORS or digest(a.units/'results.json')!=UNITS or digest(a.labels/'results.json')!=LABELS:
        raise ValueError('audited full-source captures required')
    v=json.loads((a.vectors/'results.json').read_text());u=json.loads((a.units/'results.json').read_text());lc=json.loads((a.labels/'results.json').read_text())
    capture,source,meetings=load(a.source)
    if (capture,source)!=(v['capture_sha256'],v['source_sha256']) or u['vectors_sha256']!=VECTORS:raise ValueError('source drift')
    for record in [v,u]:
        for name,pin in record['code_pins'].items():
            if digest(Path(__file__).with_name(name))!=pin:raise ValueError('frozen causal evidence implementation drift')
    path=a.vectors/'raw-vectors.npy'
    if digest(path)!=v['files'][path.name]['sha256']:raise ValueError('raw vectors drift')
    raw=np.load(path,mmap_mode='r',allow_pickle=False)
    if raw.shape!=(88336,384) or not np.isfinite(raw).all():raise ValueError('complete finite source vectors required')
    ready=np.array([all(x['status']=='ready' for x in row['variants'].values()) for row in v['rows']],dtype=bool)
    tokens=np.array([row['variants']['raw']['tokens'] for row in v['rows']],dtype=np.int64)
    if ready.shape!=(88336,) or int(ready.sum())!=84434 or u['status_counts']!={'ready':84434,'unsupported_source':3902}:raise ValueError('matched full support required')
    d=lc['matrices']['exact_markers']
    for filename,pin in d['files'].items():
        if digest(a.labels/filename)!=pin:raise ValueError('source label counts drift')
    arrays=[np.load(a.labels/f'exact_markers-{k}.npy',allow_pickle=False) for k in ['data','indices','indptr']]
    counts=sparse.csr_matrix(tuple(arrays),shape=tuple(d['shape']))
    b=json.loads((a.baseline/'results.json').read_text())
    if digest(a.baseline/'results.json')!=lc['baseline_sha256'] or digest(a.baseline/'idf.npy')!=b['idf_sha256'] or digest(a.baseline/'vocabulary.json')!=b['vocabulary_sha256']:
        raise ValueError('source IDF/vocabulary drift')
    idf=np.load(a.baseline/'idf.npy');terms=json.loads((a.baseline/'vocabulary.json').read_text())
    reference=json.loads(a.reference_manifest.read_text())
    if reference['repository']!='https://github.com/iesl/grinch' or reference['commit']!='a2747d3c5d3405cf27afc839bc28352b99c3e309':raise ValueError('pinned author reference required')
    for name,pin in reference['files'].items():
        if digest(a.reference_manifest.parent/Path(name).name)!=pin:raise ValueError('author reference drift')
    lineage=u['lineage'];summary={key:value for key,value in u['summary'].items() if key.startswith('context_')}
    del v,u,lc,b;gc.collect()
    return Inputs(capture,source,meetings,raw,ready,tokens,counts,idf,terms,lineage,summary,reference)
