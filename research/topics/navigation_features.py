"""Shared frozen-count lexical features for source and full-topic diagnostics."""
from dataclasses import dataclass
from scipy import sparse
from episode_blocks import block_counts,lexical
from label_noise import EXCLUDED


@dataclass(frozen=True)
class Features:
    sources:sparse.csr_matrix
    groups:sparse.csr_matrix

    def score(self,query):
        return (self.sources@query.T).toarray().ravel(),(self.groups@query.T).toarray().ravel()


def build(counts,idf,terms,owners,groups):
    clean=counts.multiply([t not in EXCLUDED for t in terms]).tocsr();clean.eliminate_zeros()
    sources=lexical(clean,idf).tocsr();sources.sort_indices()
    topics=lexical(block_counts(clean,owners,groups),idf).tocsr();topics.sort_indices()
    return Features(sources,topics)
