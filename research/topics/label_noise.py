"""Label-only count cleanup with exact transcript marker source offsets."""
from dataclasses import asdict,dataclass
from enum import StrEnum
import re

import numpy as np
from scipy import sparse
from sklearn.feature_extraction.text import ENGLISH_STOP_WORDS


ACKNOWLEDGEMENTS=frozenset(['yeah','okay','ok','uh','um','erm','hmm','mm','oh'])
EXCLUDED=frozenset(ENGLISH_STOP_WORDS)|ACKNOWLEDGEMENTS
MARKER=re.compile(r'\{(disfmarker|vocalsound)\}')


class Policy(StrEnum):
    RAW='raw'
    MARKERS='exact_markers'
    VOCABULARY='markers_and_label_vocabulary'


@dataclass(frozen=True)
class Marker:
    source_row:int
    term:str
    char_start:int
    char_end:int
    utf8_start:int
    utf8_end:int


def clean(counts,texts,terms):
    if (not sparse.isspmatrix_csr(counts) or counts.shape!=(len(texts),len(terms))
            or not 0<len(texts)<=100000 or terms!=sorted(set(terms))
            or not np.issubdtype(counts.dtype,np.integer) or (counts.data<0).any()
            or any(not isinstance(t,str) for t in texts)):
        raise ValueError('bounded complete original source counts and vocabulary required')
    columns={term:i for i,term in enumerate(terms)};rows=[];indices=[];values=[];ledger=[]
    for i,text in enumerate(texts):
        for match in MARKER.finditer(text):
            term=match.group(1)
            if term not in columns:raise ValueError('marker absent from pinned vocabulary')
            rows.append(i);indices.append(columns[term]);values.append(1)
            ledger.append(asdict(Marker(i,term,match.start(),match.end(),len(text[:match.start()].encode()),len(text[:match.end()].encode()))))
    removed=sparse.csr_matrix((values,(rows,indices)),shape=counts.shape,dtype=counts.dtype)
    markers=(counts-removed).tocsr();markers.eliminate_zeros();markers.sort_indices()
    if (markers.data<0).any():raise ValueError('marker contributions exceed original source counts')
    keep=np.array([term not in EXCLUDED for term in terms],dtype=np.int64)
    filtered=markers.multiply(keep).tocsr();filtered.eliminate_zeros();filtered.sort_indices()
    return {Policy.RAW:counts,Policy.MARKERS:markers,Policy.VOCABULARY:filtered},ledger
