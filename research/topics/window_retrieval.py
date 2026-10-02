"""Complete source-window ownership and signed-cosine maximum reduction."""
from dataclasses import dataclass
import hashlib

import numpy as np

from embedding_windows import Field


@dataclass(frozen=True)
class Evidence:
    document: int
    field: Field
    ordinal: int
    start: int
    end: int
    byte_start: int
    byte_end: int
    text_sha256: str


def ownership(preflight,documents):
    ids=[d.id for d in documents];result=[]
    if ids!=sorted(set(ids)):raise ValueError('complete canonical candidate identities required')
    for field in [Field.TITLE,Field.BODY]:
        records=preflight['ledgers'][field]
        if [r['source'] for r in records]!=ids:raise ValueError('complete field ownership required')
        for i,(record,doc) in enumerate(zip(records,documents)):
            text=doc.title if field==Field.TITLE else doc.text
            if hashlib.sha256(text.encode()).hexdigest()!=record['source_text_sha256']:
                raise ValueError('original field source hash drift')
            start=byte_start=0
            if not record['windows']:raise ValueError('absent field evidence; no silent source drop')
            for ordinal,w in enumerate(record['windows']):
                if (w['source']!=doc.id or w['field']!=field or w['ordinal']!=ordinal
                        or type(w['start']) is not int or type(w['end']) is not int
                        or w['start']!=start or not start<w['end']<=len(text)):
                    raise ValueError('invalid contiguous source window identity/offset')
                raw=text[start:w['end']];end=w['end'];byte_end=byte_start+len(raw.encode())
                if (w['byte_start']!=byte_start or w['byte_end']!=byte_end
                        or hashlib.sha256(raw.encode()).hexdigest()!=w['text_sha256']):
                    raise ValueError('original UTF8 window span/hash drift')
                result.append(Evidence(i,field,ordinal,start,end,byte_start,byte_end,w['text_sha256']))
                start,byte_start=end,byte_end
            if start!=len(text) or byte_start!=len(text.encode()):raise ValueError('incomplete original source coverage')
    return tuple(result)


def maximum(scores,owners,documents):
    scores=np.asarray(scores,dtype=np.float64);owners=np.asarray(owners)
    if (scores.ndim!=1 or not 1<=len(scores)<=500000 or owners.shape!=scores.shape
            or not np.issubdtype(owners.dtype,np.integer) or type(documents) is not int
            or not 1<=documents<=100000 or not np.isfinite(scores).all()
            or (np.abs(scores)>1+1e-12).any() or (owners<0).any() or (owners>=documents).any()):
        raise ValueError('bounded aligned complete cosine scores and integer ownership required')
    values=np.full(documents,-np.inf);np.maximum.at(values,owners,scores)
    winners=np.full(documents,len(scores),dtype=np.int64)
    rows=np.flatnonzero(scores==values[owners]);np.minimum.at(winners,owners[rows],rows)
    if not np.isfinite(values).all() or (winners==len(scores)).any():raise ValueError('every document requires original window evidence')
    return values,winners
