"""Fixed incremental factor-topic baseline, not a production online index."""
from collections import Counter,defaultdict
import hashlib
import json
import math
import time
import warnings
import numpy as np
from scipy import sparse
from sklearn.decomposition import MiniBatchNMF
from sklearn.preprocessing import normalize
from subject_labels import ranked_terms
from topic_features import terms


def array_hash(value):
    value=np.asarray(value,dtype='<f8',order='C')
    return hashlib.sha256(json.dumps(list(value.shape)).encode()+value.tobytes()).hexdigest()


def frame(sources):
    if len({source.id for source in sources})!=len(sources):raise ValueError('unique source identities required')
    providers={}
    for source in sources:
        if source.thread in providers and providers[source.thread]!=source.provider:raise ValueError('provider scope drift')
        providers[source.thread]=source.provider
    unit_by_key={}
    for i,source in sorted(enumerate(sources),key=lambda item:(item[1].thread,item[1].text,item[1].id)):
        unit_by_key.setdefault((source.thread,source.text),i)
    units=list(unit_by_key.values());counts=[Counter(terms(source.text)) for source in sources]
    total=Counter();threads=defaultdict(set)
    for i in units:
        total.update(counts[i])
        for term in counts[i]:threads[term].add(sources[i].thread)
    thread_count=len(providers)
    allowed=[term for term in total if any(c.isalpha() for c in term)
        and (thread_count<3 or len(threads[term])<=.4*thread_count)]
    vocabulary=sorted(sorted(allowed,key=lambda term:(-total[term],term))[:8192]);positions={term:i for i,term in enumerate(vocabulary)}
    rows=[];cols=[];values=[]
    for i,count in enumerate(counts):
        for term,value in sorted(count.items()):
            if term in positions:
                rows.append(i);cols.append(positions[term]);values.append((1+math.log(value))*(math.log((1+thread_count)/(1+len(threads[term])))+1))
    matrix=normalize(sparse.csr_matrix((values,(rows,cols)),shape=(len(sources),len(vocabulary)),dtype=np.float64),norm='l2')
    active=[i for i in units if matrix.indptr[i+1]>matrix.indptr[i]]
    return vocabulary,matrix,active,counts


def mixture_shares(coefficients,components):
    if (coefficients.ndim!=2 or components.ndim!=2 or coefficients.shape[1]!=len(components)
        or not np.isfinite(coefficients).all() or not np.isfinite(components).all()
        or np.any(coefficients<0) or np.any(components<0)):raise ValueError('aligned finite nonnegative factor matrices required')
    corrected=coefficients*np.linalg.norm(components,axis=1)
    total=corrected.sum(axis=1,keepdims=True)
    return np.divide(corrected,total,out=np.zeros_like(corrected),where=total>1e-12)


def source_candidates(shares):
    if shares.ndim!=2 or not np.isfinite(shares).all() or np.any(shares<0) or np.any(shares>1):raise ValueError('finite mixture shares required')
    result=[]
    for row in shares:
        best=float(row.max()) if len(row) else 0.
        result.append(sorted((j for j,value in enumerate(row) if value>=.25 and value>=.75*best),key=lambda j:(-row[j],j))[:3])
    return result


def snapshot(sources,vocabulary,counts,components,shares,namespace):
    candidates=source_candidates(shares);grouped=defaultdict(list)
    for i,source in enumerate(sources):grouped[source.thread].append(i)
    generation=array_hash(components);ids=['nmf-'+hashlib.sha256((namespace+'\0'+generation+'\0'+str(j)).encode()).hexdigest() for j in range(len(components))]
    support=defaultdict(set);memberships={source.id:[] for source in sources};thread_rows=[]
    for thread,indices in sorted(grouped.items()):
        units=defaultdict(set);score={}
        for i in indices:
            for j in candidates[i]:units[j].add(sources[i].text);score[j]=max(score.get(j,0.),float(shares[i,j]))
        remaining=set(units);covered=set();chosen=[];scale=max(score.values(),default=1.)
        while remaining and len(chosen)<8:
            gains={j:len(units[j]-covered) for j in remaining};largest=max(gains.values())
            utility={j:.5*score[j]/scale+(.5*gains[j]/largest if largest else 0.) for j in remaining}
            j=min(remaining,key=lambda j:(-utility[j],-score[j],j));chosen.append(j);remaining.remove(j);covered.update(units[j])
        tags=[{'topic':ids[j],'score':score[j]} for j in chosen]
        for i in indices:
            for j in chosen:
                if j in candidates[i]:
                    support[j].add(i);memberships[sources[i].id].append({'topic':ids[j],'score':float(shares[i,j])})
        thread_rows.append({'thread':thread,'provider':sources[indices[0]].provider,'sources':sorted(sources[i].id for i in indices),'tags':tags})
    topics=[]
    for j,indices in sorted(support.items()):
        observed={term for i in indices for term in counts[i]}
        scores=np.array([value if term in observed else 0. for term,value in zip(vocabulary,components[j])])
        keywords=ranked_terms(scores,vocabulary,diverse=True,limit=8)
        if not keywords:raise ValueError('assigned component lacks original lexical label evidence')
        topics.append({'id':ids[j],'component':j,'phrase':keywords[0]['term'],'keywords':keywords,
            'sources':sorted(sources[i].id for i in indices),'threads':sorted({sources[i].thread for i in indices})})
    return {'schema_version':1,'representation':'flat_incremental_nmf_baseline','namespace':namespace,
        'topics':topics,'threads':thread_rows,'memberships':memberships,
        'diagnostics':{'sources':len(sources),'threads':len(grouped),'topics':len(topics),
            'untagged_sources':sum(not tags for tags in memberships.values()),
            'multi_tag_sources':sum(len(tags)>1 for tags in memberships.values()),
            'untagged_threads':sum(not thread['tags'] for thread in thread_rows)}}


def experiment(sources,components_count,passes,reverse,namespace):
    if components_count not in {16,32} or passes not in {1,5} or type(reverse) is not bool:raise ValueError('frozen NMF arms required')
    start=time.perf_counter();cpu=time.process_time();vocabulary,matrix,active,counts=frame(sources);frame_ms=1000*(time.perf_counter()-start)
    if not active or not vocabulary:raise ValueError('nonempty lexical training frame required')
    order=active[::-1] if reverse else active
    model=MiniBatchNMF(n_components=components_count,init='random',random_state=718,batch_size=128,
        beta_loss='frobenius',forget_factor=1.,alpha_W=0.,alpha_H=0.,l1_ratio=0.,fresh_restarts=False,
        transform_max_iter=100,tol=1e-4,max_no_improvement=None)
    begin=time.perf_counter()
    with warnings.catch_warnings(record=True) as recorded:
        warnings.simplefilter('always')
        for _ in range(passes):
            for first in range(0,len(order),128):model.partial_fit(matrix[order[first:first+128]])
        fit_ms=1000*(time.perf_counter()-begin);begin=time.perf_counter()
        coefficients=model.transform(matrix);shares=mixture_shares(coefficients,model.components_)
        transform_ms=1000*(time.perf_counter()-begin)
    begin=time.perf_counter();result=snapshot(sources,vocabulary,counts,model.components_,shares,namespace);snapshot_ms=1000*(time.perf_counter()-begin)
    return {'snapshot':result,'parameters':{'components':components_count,'passes':passes,'reverse':reverse,'seed':718,
        'batch':128,'share_floor':.25,'relative_share':.75,'maximum_source_tags':3,'maximum_thread_tags':8,
        'vocabulary':'frozen transductive source terms/sublinearTF/threadIDF/L2','forget_factor':1.},
        'active_unique_training_units':len(active),'vocabulary':len(vocabulary),'steps':int(model.n_steps_),
        'vocabulary_sha256':hashlib.sha256(json.dumps(vocabulary,ensure_ascii=False,separators=(',',':')).encode()).hexdigest(),
        'components_sha256':array_hash(model.components_),'coefficients_sha256':array_hash(coefficients),
        'matrix_sha256':hashlib.sha256(matrix.indptr.astype('<i8').tobytes()+matrix.indices.astype('<i8').tobytes()+matrix.data.astype('<f8').tobytes()).hexdigest(),
        'warning_counts':dict(sorted(Counter(type(warning.message).__name__ for warning in recorded).items())),
        'runtime':{'frame_ms':frame_ms,'partial_fit_ms':fit_ms,'transform_ms':transform_ms,'snapshot_ms':snapshot_ms,
            'total_ms':1000*(time.perf_counter()-start),'cpu_ms':1000*(time.process_time()-cpu)}}
