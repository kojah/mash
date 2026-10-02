"""Audit-only count reference; scalar geometry and separate IEEE kernel replay."""
from collections import Counter
from dataclasses import dataclass
import hashlib
import json
import math
import re
import numpy as np
from scipy import sparse
from sklearn.feature_extraction.text import ENGLISH_STOP_WORDS


EXCLUDED=frozenset(ENGLISH_STOP_WORDS)|{'yeah','okay','ok','uh','um','erm','hmm','mm','oh'}
TOKEN=re.compile(r'(?u)\b\w\w+\b');MARKER=re.compile(r'\{(disfmarker|vocalsound)\}')


def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()


def features(counts,idf):
    # Separate formula replay using shared NumPy/SciPy kernels, no producer imports.
    weighted=counts.astype(float,copy=True);weighted.data=1+np.log(weighted.data)
    weighted=weighted.multiply(idf).tocsr();norm=np.sqrt(np.asarray(weighted.multiply(weighted).sum(axis=1)).ravel())
    replay=(sparse.diags(np.divide(1.,norm,out=np.zeros_like(norm),where=norm>0))@weighted).tocsr();replay.sort_indices()
    maximum=0.;coordinates=0
    for i in range(counts.shape[0]):
        row=counts[i];values={int(j):(1+math.log(float(n)))*float(idf[j]) for j,n in zip(row.indices,row.data)}
        norm=math.sqrt(math.fsum(v*v for v in values.values()))
        expected={j:v/norm for j,v in values.items()} if norm else {};actual=replay[i]
        assert set(expected)==set(actual.indices.tolist())
        for j,v in zip(actual.indices,actual.data):maximum=max(maximum,abs(expected[int(j)]-float(v)));coordinates+=1
    assert maximum<=1e-12
    return replay,coordinates,maximum


@dataclass(frozen=True)
class Reference:
    control:dict
    meetings:list
    geometry:dict
    owners:np.ndarray
    source_meeting:np.ndarray
    group_meeting:np.ndarray
    source_vectors:sparse.csr_matrix
    group_vectors:sparse.csr_matrix
    terms:list
    idf:np.ndarray
    coordinates:int
    maximum_scalar_error:float

    def scores(self,text):
        raw=Counter(TOKEN.findall(text.lower()))
        for marker in MARKER.finditer(text):raw[marker.group(1)]-=1
        assert all(n>=0 for n in raw.values());words={t:n for t,n in raw.items() if n and t not in EXCLUDED}
        positions={t:i for i,t in enumerate(self.terms)};known=sorted((positions[t],n) for t,n in words.items() if t in positions)
        counts=sparse.csr_matrix(([n for j,n in known],([0]*len(known),[j for j,n in known])),shape=(1,len(positions)))
        vector,_,maximum=features(counts,self.idf)
        return (self.source_vectors@vector.T).toarray().ravel(),(self.group_vectors@vector.T).toarray().ravel(),maximum,


def load(root,control_pin,source_pin):
    cp=root/'topic-qmsum-query-navigation-first-20261002.json';assert digest(cp)==control_pin;control=json.loads(cp.read_text())
    sp=root/'topic-qmsum-author-training-20261002/data/ALL/jsonl/train.jsonl';assert digest(sp)==source_pin
    meetings=[json.loads(line) for line in sp.read_bytes().splitlines()]
    pp=root/'topic-qmsum-source-support-first-20261002.json';assert digest(pp)==control['control_sha256'];previous=json.loads(pp.read_text())
    gp=root/'topic-qmsum-block-geometry-first-20261002/results.json';assert digest(gp)==previous['geometry_sha256'];geometry=json.loads(gp.read_text())
    lp=root/'topic-qmsum-label-cleanup-first-20261002/results.json';assert digest(lp)=='064ff3731955b515a07e12e3897102e2cd9d137ac91c43ec222e393a2e492d08'
    ledger=json.loads(lp.read_text());schema=ledger['matrices']['exact_markers']
    for name,pin in schema['files'].items():assert digest(lp.parent/name)==pin
    counts=sparse.csr_matrix(tuple(np.load(lp.parent/f'exact_markers-{k}.npy') for k in ['data','indices','indptr']),shape=tuple(schema['shape']))
    vr=root/'topic-qmsum-lexical-context-final-first-20261002';baseline=json.loads((vr/'results.json').read_text())
    assert digest(vr/'results.json')==ledger['baseline_sha256'] and digest(vr/'vocabulary.json')==baseline['vocabulary_sha256'] and digest(vr/'idf.npy')==baseline['idf_sha256']
    terms=json.loads((vr/'vocabulary.json').read_text());idf=np.load(vr/'idf.npy');del ledger,baseline
    groups=control['groups'];owners=np.full(counts.shape[0],-1,dtype=np.int64);source_meeting=np.full(len(owners),-1,dtype=np.int64)
    for ordinal,(meeting,g,r) in enumerate(zip(meetings,geometry['records'],previous['records'])):
        offset=g['source_offset'];source_meeting[offset:offset+len(meeting['meeting_transcripts'])]=ordinal
        gids=[v['id'] for v in groups if v['meeting']==ordinal];block_group={b:gid for gid,blocks in zip(gids,r['groups']) for b in blocks}
        for b in g['blocks']:
            for i in range(offset+b['start'],offset+b['end']+1):assert owners[i]==-1;owners[i]=block_group[b['ordinal']]
    assert (source_meeting>=0).all() and int((owners>=0).sum())==84434
    clean=counts.multiply([t not in EXCLUDED for t in terms]).tocsr();clean.eliminate_zeros()
    rows=np.flatnonzero(owners>=0);incidence=sparse.csr_matrix((np.ones(len(rows)),(owners[rows],rows)),shape=(len(groups),len(owners)))
    pooled=(incidence@clean).tocsr();sources,sc,sm=features(clean,idf);topics,tc,tm=features(pooled,idf)
    return Reference(control,meetings,geometry,owners,source_meeting,np.array([g['meeting'] for g in groups]),sources,topics,terms,idf,sc+tc,max(sm,tm))
