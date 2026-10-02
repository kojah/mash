"""Versioned source-only hashing candidate; no learned vocabulary or IDF."""
from collections import Counter
from dataclasses import dataclass
from enum import StrEnum
import hashlib
import json
import math
import re
import unicodedata

import numpy as np
from scipy import sparse

from label_noise import EXCLUDED,MARKER


BUCKETS=16384
DOMAIN=b'mash.topic.lexical.v1\0'
TOKEN=re.compile(r'(?u)\b\w\w+\b')


class Policy(StrEnum):
    RAW='marker_only_tf'
    FILTERED='static_noise_filtered_tf'


class State(StrEnum):
    READY='ready'
    FALLBACK='filtered_empty_fallback'
    EMPTY='empty'


@dataclass(frozen=True)
class FrameRow:
    state:State
    direction:sparse.csr_matrix|None
    original_counts:dict[str,int]
    used_counts:dict[str,int]
    frame_sha256:str


def descriptor(policy):
    policy=Policy(policy)
    return {'version':1,'buckets':BUCKETS,'hash':'sha256_domain_first8_little_endian_modulo',
        'domain_hex':DOMAIN.hex(),'token_pattern':TOKEN.pattern,'lowercase':'Python3.11_unicode_lower',
        'unicode_version':unicodedata.unidata_version,'markers':'exact_case_sensitive_counts_subtraction',
        'marker_pattern':MARKER.pattern,'marker_flags':MARKER.flags,
        'term_weight':'1+ln_count_before_bucket_sum','norm':'float64_L2_positive_finite',
        'policy':policy,'excluded':sorted(EXCLUDED) if policy==Policy.FILTERED else [],
        'empty_filtered':'raw_geometry_flagged_no_invented_labels','empty_raw':'unassigned_no_direction'}


def fingerprint(policy):
    return hashlib.sha256(json.dumps(descriptor(policy),sort_keys=True,ensure_ascii=False,separators=(',',':')).encode()).hexdigest()


def bucket(term):
    if not isinstance(term,str) or not term:raise ValueError('nonempty term required')
    return int.from_bytes(hashlib.sha256(DOMAIN+term.encode()).digest()[:8],'little')%BUCKETS


def counts(text):
    if not isinstance(text,str) or len(text)>1000000:raise ValueError('bounded source text required')
    result=Counter(TOKEN.findall(text.lower()))
    for marker in MARKER.finditer(text):
        result[marker.group(1)]-=1
    if any(v<0 for v in result.values()):raise ValueError('invalid marker accounting')
    return dict(sorted((k,v) for k,v in result.items() if v))


def encode(original,policy):
    policy=Policy(policy)
    if (not isinstance(original,dict) or len(original)>100000 or
        any(not isinstance(t,str) or not TOKEN.fullmatch(t) or t!=t.lower()
            or type(n) is not int or not 0<n<=2**53 for t,n in original.items())):
        raise ValueError('bounded positive integral term counts required')
    original=dict(sorted(original.items()));used={t:n for t,n in original.items() if policy==Policy.RAW or t not in EXCLUDED}
    state=State.READY
    if not used:
        if not original:return FrameRow(State.EMPTY,None,original,{},fingerprint(policy))
        used=original.copy();state=State.FALLBACK
    sums={}
    for t,n in sorted(used.items()):
        i=bucket(t);sums.setdefault(i,[]).append(1+math.log(n))
    ids=sorted(sums);values=[math.fsum(sums[i]) for i in ids]
    norm=math.sqrt(math.fsum(v*v for v in values))
    if not math.isfinite(norm) or norm<=0:raise ValueError('positive finite frame norm required')
    direction=sparse.csr_matrix((np.asarray(values)/norm,([0]*len(ids),ids)),shape=(1,BUCKETS))
    return FrameRow(state,direction,original,used,fingerprint(policy))


def require_frame(expected,actual):
    if not isinstance(expected,str) or not re.fullmatch('[0-9a-f]{64}',expected) or expected!=actual:raise ValueError('mixed or invalid lexical frame')
