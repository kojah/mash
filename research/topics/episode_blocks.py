"""Bounded online source blocks: representation control, not semantic episodes."""
from dataclasses import dataclass
from enum import StrEnum

import numpy as np
from scipy import sparse


class End(StrEnum):
    TURNS = 'sixteen_turn_limit'
    TOKENS = 'original_model_token_limit'
    GAP = 'unsupported_source_gap'
    MEETING = 'meeting_end'


@dataclass(frozen=True)
class Block:
    ordinal: int
    start: int
    end: int
    tokens: int
    close: End
    cancelled: bool


def pool(vectors,ready,tokens):
    vectors = np.asarray(vectors,dtype=np.float64);ready = np.asarray(ready);tokens = np.asarray(tokens)
    if (vectors.ndim != 2 or not 0 < len(vectors) <= 2048 or not 0 < vectors.shape[1] <= 20000
            or ready.shape != (len(vectors),) or ready.dtype != np.bool_ or tokens.shape != ready.shape
            or not np.issubdtype(tokens.dtype,np.integer) or (tokens < 2).any() or (tokens[ready] > 512).any()
            or not np.isfinite(vectors).all() or not np.allclose(np.linalg.norm(vectors[ready],axis=1),1,rtol=0,atol=1e-12)):
        raise ValueError('bounded finite immutable directions/support/model tokens required')
    blocks = [];owner = np.full(len(vectors),-1,dtype=np.int64);directions = [];start = None;total = 0;accumulated = None
    def close(end,reason):
        nonlocal start,total,accumulated
        norm = float(np.linalg.norm(accumulated));cancelled = norm <= 1e-12;ordinal = len(blocks)
        directions.append(np.zeros(vectors.shape[1]) if cancelled else accumulated/norm)
        blocks.append(Block(ordinal,start,end,total,reason,cancelled));owner[start:end+1] = ordinal
        start = None;total = 0;accumulated = None
    for i in range(len(vectors)):
        if not ready[i]:
            if start is not None:close(i-1,End.GAP)
            continue
        if start is not None:
            if i-start == 16:close(i-1,End.TURNS)
            elif total+int(tokens[i]) > 512:close(i-1,End.TOKENS)
        if start is None:start = i;accumulated = np.zeros(vectors.shape[1],dtype=np.float64)
        accumulated += vectors[i];total += int(tokens[i])
    if start is not None:close(len(vectors)-1,End.MEETING)
    return blocks,owner,np.asarray(directions,dtype=np.float64).reshape(len(blocks),vectors.shape[1])


def lexical(counts,idf):
    if (not sparse.isspmatrix_csr(counts) or not np.isfinite(counts.data).all() or (counts.data <= 0).any()
            or np.asarray(idf).shape != (counts.shape[1],) or not np.isfinite(idf).all() or (np.asarray(idf) <= 0).any()):
        raise ValueError('positive original CSR counts and frozen finite IDF required')
    matrix = counts.astype(np.float64,copy=True);matrix.data = 1+np.log(matrix.data)
    matrix = matrix.multiply(idf).tocsr();norm = np.sqrt(np.asarray(matrix.multiply(matrix).sum(axis=1)).ravel())
    return sparse.diags(np.divide(1.,norm,out=np.zeros_like(norm),where=norm>0)) @ matrix


def block_counts(counts,owner,block_count):
    rows = np.flatnonzero(owner>=0)
    incidence = sparse.csr_matrix((np.ones(len(rows)),(owner[rows],rows)),shape=(block_count,len(owner)))
    return (incidence @ counts).tocsr()
