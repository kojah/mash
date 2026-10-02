"""Bounded initial source calibration, immutable weights and canonical sparse rows."""
from collections import Counter
from dataclasses import dataclass
import hashlib
import json
import math
import re
from types import MappingProxyType
import numpy as np
from scipy import sparse
from stable_lexical_frame import Policy,FrameRow,BUCKETS,TOKEN,bucket,descriptor,encode


def pin(value):
    return hashlib.sha256(json.dumps(value,sort_keys=True,ensure_ascii=False,separators=(',',':'),allow_nan=False).encode()).hexdigest()


@dataclass(frozen=True)
class Model:
    policy:Policy
    documents:int
    frequencies:tuple[tuple[str,int],...]
    seed_sha256:str

    def description(self):
        return {'version':1,'encoding':descriptor(self.policy),'seed_documents':self.documents,
            'term_document_frequencies':dict(self.frequencies),'seed_sha256':self.seed_sha256,
            'idf':'ln((1+seed_documents)/(1+term_df))+1_unseen_df0',
            'weighted_norm':'sorted_term_scalar_weights_bucket_fsum_L2_fsum',
            'csr_layout':'ascending_unique_indices'}


def calibrate(sources,identities,policy):
    policy=Policy(policy)
    if (not isinstance(sources,list) or not 1<=len(sources)<=32 or len(identities)!=len(sources)
            or len(set(identities))!=len(identities)
            or any(not isinstance(i,str) or not i or len(i)>256 for i in identities)):
        raise ValueError('bounded unique bootstrap source identities required')
    frequencies=Counter();ledger=[]
    for source,identity in zip(sources,identities):
        row=encode(source,policy);frequencies.update(row.used_counts.keys())
        ledger.append({'id':identity,'original_counts':row.original_counts,'state':row.state})
    return Model(policy,len(sources),tuple(sorted(frequencies.items())),pin(ledger))


def prepare(model,terms=None):
    if (not isinstance(model,Model) or type(model.documents) is not int or not 1<=model.documents<=32
            or not isinstance(model.seed_sha256,str) or not re.fullmatch('[0-9a-f]{64}',model.seed_sha256)
            or tuple(sorted(set(model.frequencies)))!=model.frequencies
            or len({t for t,_ in model.frequencies})!=len(model.frequencies)
            or any(not isinstance(t,str) or not TOKEN.fullmatch(t) or t!=t.lower()
                or type(df) is not int or not 1<=df<=model.documents for t,df in model.frequencies)):
        raise ValueError('immutable bounded source model required')
    kind='hashed' if terms is None else 'identity_oracle'
    frame=pin({'model':model.description(),'kind':kind})
    coordinates=None
    if terms is not None:
        if not isinstance(terms,list) or not 1<=len(terms)<=20000 or terms!=sorted(set(terms)):
            raise ValueError('complete canonical term identity oracle required')
        coordinates=MappingProxyType({t:i for i,t in enumerate(terms)})
    return Encoder(model,MappingProxyType(dict(model.frequencies)),coordinates,BUCKETS if terms is None else len(terms),frame)


@dataclass(frozen=True)
class Encoder:
    model:Model
    frequencies:MappingProxyType
    coordinates:MappingProxyType|None
    dimensions:int
    frame_sha256:str

    def encode(self,original):
        row=encode(original,self.model.policy)
        if row.direction is None:return FrameRow(row.state,None,row.original_counts,row.used_counts,self.frame_sha256)
        if self.coordinates is not None and any(t not in self.coordinates for t in row.used_counts):
            raise ValueError('source term absent from identity oracle')
        values={}
        for term,count in sorted(row.used_counts.items()):
            weight=(1+math.log(count))*(math.log((1+self.model.documents)/(1+self.frequencies.get(term,0)))+1)
            index=bucket(term) if self.coordinates is None else self.coordinates[term]
            values.setdefault(index,[]).append(weight)
        ids=sorted(values);weights=[math.fsum(values[i]) for i in ids];norm=math.sqrt(math.fsum(v*v for v in weights))
        if not math.isfinite(norm) or norm<=0:raise ValueError('positive finite calibrated norm required')
        direction=sparse.csr_matrix((np.array(weights)/norm,([0]*len(ids),ids)),shape=(1,self.dimensions))
        direction.sort_indices()
        return FrameRow(row.state,direction,row.original_counts,row.used_counts,self.frame_sha256)


def weighted_row(original,model,terms=None):
    return prepare(model,terms).encode(original)
