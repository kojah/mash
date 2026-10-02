"""Frozen training-word frame and source-only validation hierarchy inventory."""
from collections import Counter
from dataclasses import dataclass
import hashlib
import json
from pathlib import Path
import numpy as np
from scipy import sparse
from qmsum_validation_capture import load, ValidationSource
from episode_blocks import lexical, block_counts
from stable_lexical_frame import counts
from stable_frame_quality import label_cut
from block_hierarchy import average


UNITS = '6192cf134f0d49c54fc9b367a9c80745edfee1735f404984dcf7312bfb665c1c'
BASELINE = 'e3c89677f538f65f3e23a5a7de14de6ef9340febb40a408d97ebb275cb11b488'


def digest(path): return hashlib.sha256(path.read_bytes()).hexdigest()


@dataclass(frozen=True)
class Inventory:
    source: ValidationSource
    terms: list[str]
    idf: np.ndarray
    counts: sparse.csr_matrix
    ready: np.ndarray
    topic_owner: np.ndarray
    source_meeting: np.ndarray
    topic_meeting: np.ndarray
    records: list[dict]
    support: dict
    inputs: dict


def build(root):
    source = load(root/'topic-qmsum-author-validation-20261002')
    up = root/'qmsum-validation-units-first-20261002.json'
    if digest(up) != UNITS: raise ValueError('frozen original source units required')
    units = json.loads(up.read_text())
    if units['source_sha256'] != source.source_sha256: raise ValueError('unit/source drift')
    for name,pin in units['code_pins'].items():
        if digest(Path(__file__).with_name(name)) != pin: raise ValueError('unit implementation drift')
    folder = root/'topic-qmsum-lexical-context-final-first-20261002'; bp = folder/'results.json'
    if digest(bp) != BASELINE: raise ValueError('frozen training word frame required')
    baseline = json.loads(bp.read_text()); vp = folder/'vocabulary.json'; ip = folder/'idf.npy'
    if digest(vp) != baseline['vocabulary_sha256'] or digest(ip) != baseline['idf_sha256']: raise ValueError('training frame drift')
    terms = json.loads(vp.read_text()); idf = np.load(ip)
    if terms != sorted(set(terms)) or len(terms) != 18316 or idf.shape != (18316,): raise ValueError('complete original word frame required')
    positions = {t:i for i,t in enumerate(terms)}; rows = []; cols = []; values = []; stats = Counter()
    offsets = []; offset = 0
    # Only original source text enters this loop; annotations/queries are unused.
    for meeting in source.meetings:
        offsets.append(offset)
        for turn in meeting.turns:
            words = counts(turn.text); known = sorted((positions[t],n) for t,n in words.items() if t in positions)
            rows.extend([offset]*len(known)); cols.extend(j for j,n in known); values.extend(n for j,n in known)
            stats.update(source_unique_terms=len(words),source_oov_unique_terms=sum(t not in positions for t in words),
                         source_oov_occurrences=sum(n for t,n in words.items() if t not in positions),source_zero_known=int(not known))
            offset += 1
    matrix = sparse.csr_matrix((values,(rows,cols)),shape=(offset,len(terms)),dtype=np.float64); matrix.sort_indices()
    ready = np.array([r['ready'] for r in units['rows']],dtype=bool)
    owner = np.full(offset,-1,dtype=np.int64); source_meeting = np.full(offset,-1,dtype=np.int64); records = []; group_meetings = []
    for meeting,u,start in zip(source.meetings,units['meetings'],offsets):
        n = len(meeting.turns); local_owner = np.array(u['owners'],dtype=np.int64); source_meeting[start:start+n] = meeting.ordinal
        if u['source_offset'] != start or not np.array_equal(local_owner >= 0,ready[start:start+n]): raise ValueError('original ready ownership drift')
        bc = block_counts(matrix[start:start+n],local_owner,len(u['blocks'])); features = lexical(bc,idf).tocsr()
        groups = []; tree_payload = None; merges = []
        if len(u['blocks']):
            tree,merges = average((features@features.T).toarray())
            tree_payload = {'root':tree.root,'children':{str(k):list(v) for k,v in tree.children.items()},'leaves':{str(k):v for k,v in tree.leaves.items()}}
            for members in label_cut(tree):
                gid = len(group_meetings); group_meetings.append(meeting.ordinal)
                selected = np.isin(local_owner,members); owner[start:start+n][selected] = gid
                groups.append({'id':gid,'blocks':members,'original_ready_sources':(start+np.flatnonzero(selected)).tolist()})
        records.append({'meeting':meeting.ordinal,'source_offset':start,'tree':tree_payload,'merges':merges,'groups':groups,
                        'zero_raw_lexical_blocks':int((features.getnnz(axis=1) == 0).sum())})
    if offset != 20123 or not np.array_equal(owner >= 0,ready) or (source_meeting < 0).any(): raise ValueError('complete original inventory required')
    stats.update(sources=offset,ready=int(ready.sum()),unsupported=int((~ready).sum()),blocks=units['support']['blocks'],
                 topic_parents=len(group_meetings),thread_parents=len(source.meetings))
    return Inventory(source,terms,idf,matrix,ready,owner,source_meeting,np.array(group_meetings,dtype=np.int64),records,dict(stats),
        {'units_sha256':UNITS,'training_frame_sha256':BASELINE,'vocabulary_sha256':digest(vp),'idf_sha256':digest(ip)})
