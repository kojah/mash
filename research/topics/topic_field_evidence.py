"""Complete candidate-only source loading and fixed-vocabulary field evidence."""
from enum import StrEnum

import numpy as np
from sklearn.feature_extraction.text import CountVectorizer

from cqadup_semantic import digest
from cqadup_source import parse_source
from topic_labels import analyzer
from topic_review import decode


class Field(StrEnum):
    TITLE='title'
    BODY='body'


def candidates(root, capture_pin, identities):
    if digest(root/'capture.json')!=capture_pin: raise ValueError('pinned candidate capture required')
    capture=decode((root/'capture.json').read_bytes());path=root/'corpus.jsonl';pin=capture['files']['corpus.jsonl']
    if (not capture['member_crc_verified'] or digest(path)!=pin['sha256']
            or path.stat().st_size!=pin['size']): raise ValueError('complete verified candidate source bytes required')
    documents={}
    with path.open('rb') as f:
        while raw:=f.readline(4*1024*1024+1):
            if len(raw)>4*1024*1024 or len(documents)>=100000: raise ValueError('source budget exceeded; no truncation')
            doc=parse_source(decode(raw))
            if doc.id in documents: raise ValueError('duplicate candidate identity')
            documents[doc.id]=doc
    if sorted(documents)!=identities: raise ValueError('complete canonical candidate identities required')
    return [documents[i] for i in identities]


def counts(documents, terms, field):
    if (not isinstance(field,Field) or not documents or len(documents)>100000
            or not terms or terms!=sorted(set(terms)) or len(terms)>32768):
        raise ValueError('bounded source corpus, canonical fixed vocabulary and typed field required')
    vectorizer=CountVectorizer(analyzer=analyzer,vocabulary={t:i for i,t in enumerate(terms)},dtype=np.float64)
    result=vectorizer.transform(doc.title if field==Field.TITLE else doc.text for doc in documents).tocsr()
    result.sum_duplicates();result.eliminate_zeros();result.sort_indices()
    return result
