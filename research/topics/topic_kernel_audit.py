"""Independent frozen-source scalar lexical weighting and flat output audit."""
import argparse
import json
import math
from pathlib import Path
import numpy as np
from scipy import sparse
from sklearn.feature_extraction.text import CountVectorizer
from topic_capacity_audit import counts
from topic_flat_audit import audit


def check(source_baseline,first,repeat):
    result=audit(source_baseline,first,repeat,('dense','lexical','half_fusion'),False)
    rows=json.loads(source_baseline.read_text())['baseline_checkpoint']['records']
    rows=sorted(rows,key=lambda row:row['id']);n=len(rows)
    vectorizer=CountVectorizer(analyzer=lambda text:counts(text).elements(),max_features=8192)
    matrix=vectorizer.fit_transform(row['text'] for row in rows).tocsr()
    df=np.asarray((matrix>0).sum(axis=0)).ravel();vocabulary=vectorizer.get_feature_names_out()
    idf=[math.log((1+n)/(1+int(value)))+1 for value in df]
    weights=matrix.astype(float)
    for i in range(n):
        start,end=weights.indptr[i:i+2]
        values=[(1+math.log(float(value)))*idf[int(j)] for value,j in zip(weights.data[start:end],weights.indices[start:end])]
        norm=math.sqrt(math.fsum(value*value for value in values))
        weights.data[start:end]=[value/norm for value in values] if norm else []
    dense=np.array([row['vector'] for row in rows]);dense_kernel=dense@dense.T
    lexical_kernel=(weights@weights.T).toarray();combined=(dense_kernel+lexical_kernel)*.5
    scale=np.sqrt(np.outer(np.diag(combined),np.diag(combined)));combined/=scale
    edge_counts={}
    for name,kernel in [('dense',dense_kernel),('lexical',lexical_kernel),('half_fusion',combined)]:
        edge_counts[name]=int(np.count_nonzero(np.triu(kernel>=.55,k=1)))
    captures=json.loads(first.read_text())
    zero=int(np.count_nonzero(matrix.getnnz(axis=1)==0))
    assert captures['arms']['lexical']['forward_unroutable']==captures['arms']['lexical']['reverse_unroutable']==zero
    assert captures['policy']['maximum_vocabulary']==len(vocabulary)
    result.update({'scope':'flat scalar labels/source unions plus independently scalar-weighted fixed-frame pair-density diagnostic',
        'lexical_zero_sources':zero,'source_pair_edges_at_fixed_threshold':edge_counts,
        'shared_dependencies':'sklearn English stoplist/max-feature selection and NumPy/SciPy Gram products',
        'routing_decisions_independently_replayed':False,'frame_is_transductive':True})
    return result


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['source_baseline','first','repeat']:parser.add_argument('--'+name.replace('_','-'),type=Path,required=True)
    args=parser.parse_args();print(json.dumps(check(args.source_baseline,args.first,args.repeat),sort_keys=True))
