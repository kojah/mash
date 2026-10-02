"""Frozen complete-corpus BIRCH routing/label control, never production selection."""
import argparse
from dataclasses import asdict
import hashlib
import importlib.metadata
import json
from pathlib import Path
import platform
import resource
import time

import numpy as np
from scipy import sparse
from sklearn.metrics import adjusted_rand_score

from cqadup_semantic import digest
from cqadup_source import load
from external_context import compare
from topic_labels import classes, labels, support, vocabulary
from topic_geometry import SEED as BOOTSTRAP_SEED, bootstrap, diagnostics, directions
from topic_routing import DiscoveryBudget, Geometry, Membership, discover, memberships, posting_lists, route, routed_metrics


SEMANTIC = 'd6fec0c4f7fba4023487e2fc217cdf25820ea79d480bced0c370371bef350fb3'
REPEAT = '6c3b9cd9471b514270b31b6df4561545bac45511f9aad9298b27457601627109'
PREFLIGHT = '85d35491d209b95b9a6a385d8c8b4dd2d325e305e5f7cf85bb32468c44a87d0a'
SAMPLE_SEED = 'mash-topic-routing-order-v1-20261002'


def save_sparse(matrix, prefix, destination, files):
    for name in ('data', 'indices', 'indptr'):
        path = destination/(prefix+'-'+name+'.npy')
        np.save(path, getattr(matrix, name), allow_pickle=False)
        files[path.name] = digest(path)
    return list(matrix.shape)


def source_support(terms, counts, descriptions, members, documents):
    inverted = counts.tocsc()
    positions = {term: i for i, term in enumerate(terms)}
    postings = posting_lists(members, len(descriptions))
    result = []
    for topic, description in enumerate(descriptions):
        member_ids = {doc for doc, _ in postings[topic]}
        evidence = {}
        for term in sorted({entry['term'] for ranking in description.values() for entry in ranking}):
            col = positions[term]
            owners = [int(i) for i in inverted.indices[inverted.indptr[col]:inverted.indptr[col+1]] if i in member_ids]
            if not owners:
                raise ValueError('positive label without a source member')
            evidence[term] = {'supporting_documents': len(owners), 'first_source': support(documents[owners[0]], term)}
        representatives = []
        for doc, affinity in sorted(postings[topic], key=lambda pair: (-pair[1], pair[0]))[:3]:
            source = documents[doc]
            excerpt = source.text[:320]
            representatives.append({'document': source.id, 'affinity': affinity,
                                    'title': source.title, 'body_excerpt': excerpt,
                                    'excerpt_start': 0, 'excerpt_end': len(excerpt),
                                    'excerpt_byte_start': 0, 'excerpt_byte_end': len(excerpt.encode()),
                                    'source_sha256': source.content_sha256})
        result.append({'documents': len(postings[topic]), 'labels': description,
                       'label_source_support': evidence, 'representatives': representatives})
    return result


def experiment(root, semantic, repeat, preflight_path, destination, geometry=Geometry.UNIT):
    if not isinstance(geometry,Geometry):
        raise ValueError('explicit typed topic geometry required')
    started = time.perf_counter()
    if (digest(semantic/'results.json') != SEMANTIC or digest(repeat/'results.json') != REPEAT
            or digest(preflight_path) != PREFLIGHT):
        raise ValueError('frozen verified semantic inputs required')
    reference = json.loads((semantic/'results.json').read_text())
    repeated = json.loads((repeat/'results.json').read_text())
    compare(reference, repeated)
    for name, sha in reference['files'].items():
        if digest(semantic/name) != sha or digest(repeat/name) != sha:
            raise ValueError('complete semantic vectors/scores changed')
    pref = json.loads(preflight_path.read_text())
    documents, queries, refs, _ = load(root, reference['capture_sha256'])
    ids = sorted(documents)
    if ids != reference['document_ids'] or sorted(q['source'] for q in pref['ledgers']['query']) != reference['query_ids']:
        raise ValueError('source/preflight query identity alignment changed')
    docs = [documents[i] for i in ids]
    vectors = np.load(semantic/'balanced-vectors.npy', mmap_mode='r', allow_pickle=False)
    query_vectors = np.load(semantic/'query-vectors.npy', mmap_mode='r', allow_pickle=False)
    units = np.load(semantic/'unit-vectors.npy', mmap_mode='r', allow_pickle=False)
    owners = []
    for field in ('title','body'):
        if [record['source'] for record in pref['ledgers'][field]] != ids:
            raise ValueError('complete field ownership changed')
        for i, record in enumerate(pref['ledgers'][field]):
            text = docs[i].title if field == 'title' else docs[i].text
            if hashlib.sha256(text.encode()).hexdigest() != record['source_text_sha256']:
                raise ValueError('original field source hash changed')
            if any(w['source'] != ids[i] or w['field'] != field or w['ordinal'] != j
                   for j,w in enumerate(record['windows'])):
                raise ValueError('window source identity/order changed')
            owners.extend([i]*len(record['windows']))
    corpus_units = units[:len(owners)]
    if len(owners)+len(reference['query_ids']) != len(units):
        raise ValueError('complete window ownership required')
    relevant = {q: {ref.document for ref in refs if ref.query == q} for q in reference['query_ids']}
    setup_ms = (time.perf_counter()-started)*1000
    begin = time.perf_counter()
    terms, counts = vocabulary(docs)
    label_vocabulary_ms = (time.perf_counter()-begin)*1000
    files = {}
    count_shape = save_sparse(counts, 'source-counts', destination, files)
    sample = sorted(range(len(ids)), key=lambda i: hashlib.sha256((SAMPLE_SEED+'\0'+ids[i]).encode()).hexdigest())[:512]
    discovery_vectors,assignment_vectors,assignment_units,routing_queries=vectors,vectors,corpus_units,query_vectors
    geometry_report={'mode':str(geometry)}
    projection_ms=0
    if geometry == Geometry.PCA:
        begin=time.perf_counter()
        projection=bootstrap(vectors,ids)
        projected=projection.transform(vectors)
        assignment_vectors,valid_docs,norms=directions(projected)
        projected_units=projection.transform(corpus_units)
        assignment_units,valid_units,unit_norms=directions(projected_units)
        projected_queries=projection.transform(query_vectors)
        routing_queries,valid_queries,query_norms=directions(projected_queries)
        discovery_vectors=projected
        for suffix,matrix in [('mean',projection.mean),('components',projection.components),
                              ('variance-ratio',projection.variance_ratio),('document-coordinates',projected),
                              ('document-directions',assignment_vectors),('window-coordinates',projected_units),
                              ('window-directions',assignment_units),('query-coordinates',projected_queries),
                              ('query-directions',routing_queries)]:
            path=destination/('projection-'+suffix+'.npy')
            np.save(path,matrix,allow_pickle=False);files[path.name]=digest(path)
        geometry_report.update(bootstrap_seed=BOOTSTRAP_SEED,bootstrap_ids=[ids[i] for i in projection.bootstrap],
                               components=25,solver='full-SVD',whiten=False,
                               explained_variance_ratio_sum=float(projection.variance_ratio.sum()),
                               document_norm_p50=float(np.median(norms)),document_norm_p95=float(np.percentile(norms,95)),
                               document_norm_min=float(norms.min()),document_norm_max=float(norms.max()),
                               document_mean_squared_norm=float(np.mean(norms**2)),
                               zero_document_ids=[ids[i] for i in np.flatnonzero(~valid_docs)],
                               zero_window_rows=np.flatnonzero(~valid_units).tolist(),
                               zero_query_ids=[reference['query_ids'][i] for i in np.flatnonzero(~valid_queries)],
                               pair_distortion=diagnostics(vectors,projected,assignment_vectors,sample))
        projection_ms=(time.perf_counter()-begin)*1000
    captures, order_members, timings = [], [], {}
    for reverse in (False, True):
        name = 'reverse' if reverse else 'forward'
        begin = time.perf_counter()
        try:
            raw, centers, topic_ids, trace = discover(discovery_vectors, reverse=reverse,geometry=geometry)
        except DiscoveryBudget as failure:
            timings[name] = {'fit_ms': (time.perf_counter()-begin)*1000}
            captures.append({'order': name, 'status': 'discovery-budget-exceeded',
                             'maximum_centroids': failure.maximum, 'discovery_trace': failure.trace,
                             'unprocessed_documents': len(ids)-failure.trace[-1]['inserted'], 'modes': {}})
            order_members.append(None)
            print(json.dumps({'stage': 'discovery-budget-exceeded', 'order': name,
                              **failure.trace[-1]}),flush=True)
            continue
        fit_ms = (time.perf_counter()-begin)*1000
        print(json.dumps({'stage': 'discovery-complete', 'order': name, 'centroids': len(centers)}), flush=True)
        for suffix, matrix in [('raw-centroids', raw), ('centroids', centers)]:
            path = destination/(name+'-'+suffix+'.npy')
            np.save(path, matrix, allow_pickle=False); files[path.name] = digest(path)
        begin = time.perf_counter()
        members = memberships(assignment_vectors, centers, assignment_units, np.asarray(owners, dtype=np.int64))
        assignment_ms = (time.perf_counter()-begin)*1000
        order_members.append(members)
        modes = {}
        label_times, route_times = {}, {}
        for mode in Membership:
            selected = members[mode]
            posts = posting_lists(selected, len(centers))
            begin = time.perf_counter()
            aggregate = classes(counts, selected, len(centers))
            descriptors = labels(aggregate, counts, terms)
            descriptions = source_support(terms, counts, descriptors, selected, docs)
            label_times[str(mode)] = (time.perf_counter()-begin)*1000
            shape = save_sparse(aggregate, name+'-'+str(mode)+'-classes', destination, files)
            records, latencies = {}, []
            for qi, q in enumerate(reference['query_ids']):
                begin = time.perf_counter()
                topics, candidates, union, visits = route(routing_queries[qi], centers, posts)
                chosen = [i for i, _ in candidates]
                scores = vectors[chosen] @ query_vectors[qi]
                record = routed_metrics(candidates, scores, ids, relevant[q])
                latencies.append((time.perf_counter()-begin)*1000)
                records[q] = {**record, 'topics': [asdict(m) for m in topics],
                              'candidate_documents': [{'id': ids[i], 'routing_score': score} for i, score in candidates],
                              'union_before_cap': union, 'posting_visits': visits}
            route_times[str(mode)] = {'observations_ms': latencies, 'p50_ms': float(np.median(latencies)), 'p95_ms': float(np.percentile(latencies,95))}
            metrics = {key: float(np.mean([r['metrics'][key] for r in records.values()]))
                       for key in ('mrr10','ndcg10','recall10','recall100')}
            modes[str(mode)] = {'metrics': metrics, 'candidate_recall': float(np.mean([r['candidate_recall'] for r in records.values()])),
                                'query_records': records, 'memberships': [[asdict(m) for m in row] for row in selected],
                                'assigned_documents': sum(bool(row) for row in selected),
                                'multiple_topic_documents': sum(len(row)>1 for row in selected),
                                'class_counts_shape': shape, 'class_counts_nnz': aggregate.nnz,
                                'topics': descriptions}
            print(json.dumps({'stage': 'control-complete', 'order': name, 'mode': str(mode), 'metrics': metrics,
                              'candidate_recall': modes[str(mode)]['candidate_recall']}), flush=True)
        captures.append({'order': name, 'status': 'complete', 'topic_ids': topic_ids, 'discovery_trace': trace, 'modes': modes})
        timings[name] = {'fit_ms': fit_ms, 'assignment_ms': assignment_ms, 'labels_ms': label_times, 'routing': route_times}
    stability = {}
    for mode in Membership:
        if any(rows is None for rows in order_members):
            break
        a,b = (rows[mode] for rows in order_members)
        intersection,union,disagree = 0,0,0
        for pos,i in enumerate(sample):
            for j in sample[pos+1:]:
                x=bool({m.topic for m in a[i]} & {m.topic for m in a[j]})
                y=bool({m.topic for m in b[i]} & {m.topic for m in b[j]})
                intersection += x and y;union += x or y;disagree += x != y
        stability[str(mode)] = {'sample_pairs': len(sample)*(len(sample)-1)//2,
                                'coassigned_intersection': intersection, 'coassigned_union': union,
                                'coassignment_jaccard': intersection/union if union else None,
                                'different_pair_decisions': disagree}
        if mode == Membership.HARD:
            stability[str(mode)]['full_corpus_adjusted_rand'] = float(adjusted_rand_score([row[0].topic if row else -1 for row in a],[row[0].topic if row else -1 for row in b]))
    controls = {}
    lexical = json.loads((root.parent/'topic-cqadup-retrieval-first-20261002/results.json').read_text())
    if digest(root.parent/'topic-cqadup-retrieval-first-20261002/results.json') != reference['lexical_sha256']:
        raise ValueError('frozen lexical controls required')
    for method, source in [('balanced-cosine',reference),('word-bm25',lexical)]:
        records = source['results'][method]['records']
        controls[method] = {'metrics': source['results'][method]['metrics'],
                            'candidate_recall1000': float(np.mean([sum(rank<=1000 for rank in records[q]['relevant_ranks'].values())/len(records[q]['relevant_ranks']) for q in reference['query_ids']]))}
    timings.update(setup_ms=setup_ms, label_vocabulary_ms=label_vocabulary_ms,projection_ms=projection_ms,
                   total_ms=(time.perf_counter()-started)*1000,
                   peak_rss_kib=resource.getrusage(resource.RUSAGE_SELF).ru_maxrss)
    return {'schema_version': 1, 'scope': 'development-complete-corpus-automatic-topic-routing-not-agent-navigation',
            'semantic_sha256': SEMANTIC, 'repeat_sha256': REPEAT, 'preflight_sha256': PREFLIGHT,
            'source_capture_sha256': reference['capture_sha256'], 'cohort_sha256': reference['cohort_sha256'],
            'geometry':geometry_report,
            'document_ids': ids, 'query_ids': reference['query_ids'], 'reserved_queries_scored': 0, 'inference_calls': 0,
            'parameters': {'radius': .55,'branching_factor':50,'partial_fit_batch':256,'centroid_cap':8192,
                           'absolute_cosine':.55,'relative_best':.85,'topic_budget':3,'candidate_budget':1000,
                           'label_min_df':2,'label_max_df':.4,'label_vocabulary_cap':32768,'label_token_max_characters':128,
                           'class_weight':'equal document mass across retained memberships','ctfidf_average':'all topic rows including empty'},
            'python': platform.python_version(), 'versions': {name: importlib.metadata.version(name) for name in ('numpy','scipy','scikit-learn')},
            'code_pins': {name: digest(Path(__file__).with_name(name)) for name in
                          ('cqadup_topics.py','topic_routing.py','topic_geometry.py','topic_labels.py','subject_labels.py','cqadup_source.py',
                           'cqadup_semantic.py','external_context.py','uv.lock')},
            'vocabulary': terms, 'source_counts_shape': count_shape, 'source_counts_nnz': counts.nnz,
            'files': files, 'order_sample_ids': [ids[i] for i in sample], 'order_stability': stability,
            'direct_controls': controls, 'captures': captures, 'timings': timings}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('root','semantic','repeat','preflight','output-directory'):
        parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--compare',type=Path)
    parser.add_argument('--geometry',choices=list(Geometry),type=Geometry,default=Geometry.UNIT)
    args = parser.parse_args()
    if args.output_directory.exists() or args.output_directory.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git topic capture required')
    args.output_directory.mkdir()
    result = experiment(args.root,args.semantic,args.repeat,args.preflight,args.output_directory,args.geometry)
    # Validate the emitted schema's actual JSON round trip before comparing.
    result = json.loads(json.dumps(result,allow_nan=False))
    if args.compare: compare(json.loads(args.compare.read_text()),result)
    (args.output_directory/'results.json').write_text(json.dumps(result,sort_keys=True,allow_nan=False))
    print(json.dumps({'stage':'complete','order_stability': result['order_stability'],
                      'total_ms':result['timings']['total_ms'],'inference_calls':0}),flush=True)


if __name__ == '__main__': main()
