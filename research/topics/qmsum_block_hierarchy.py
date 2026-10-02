"""Offline, cut-free lexical/dense block macro reference; not an online selection."""
import argparse
from collections import Counter
import json
import math
from pathlib import Path
import resource
import time

import numpy as np
import scipy
from scipy.cluster.hierarchy import linkage
from threadpoolctl import threadpool_limits

from block_hierarchy import average,evaluate
from captured_hierarchy import Baseline,temporal
from discovery_inputs import read,digest
from episode_blocks import block_counts,lexical
from qmsum_hierarchy import components
from qmsum_unique_blocks import GEOMETRY


METHODS=['lexical_average','dense_average',*Baseline]


def stored_tree(tree):
    # Freeze the JSON key types before comparison: JSON object IDs are strings.
    return {'root':tree.root,'children':{str(k):list(v) for k,v in tree.children.items()},
            'leaves':{str(k):v for k,v in tree.leaves.items()}}


def summarize(records):
    result={}
    for split in ['all','development','reserved_internal']:
        selected=[r for r in records if split=='all' or r['split']==split];methods={}
        for method in METHODS:
            subset=[r for r in selected if r['method']==method];scopes={}
            for scope in ['all','far','crossspan']:
                values=[r['metrics'][scope] for r in subset];pairs=sum(v['pairs'] for v in values)
                weight=math.fsum(v['purity_sum'] for v in values);root=math.fsum(v['root_only_purity_sum'] for v in values)
                topics=[t for v in values for t in v['topics'] if t['pairs']]
                scopes[scope]={'pairs':pairs,'purity_sum':weight,'purity':weight/pairs if pairs else None,
                    'root_only_purity':root/pairs if pairs else None,'above_root':(weight-root)/pairs if pairs else None,
                    'supported_topics':len(topics),'supported_meetings':sum(v['pairs']>0 for v in values),
                    'topic_macro_purity':math.fsum(t['purity'] for t in topics)/len(topics) if topics else None,
                    'mean_lca_known_rows':sum(v['lca_known_rows_sum'] for v in values)/pairs if pairs else None,
                    'root_lca_pair_fraction':sum(v['root_lca_pairs'] for v in values)/pairs if pairs else None}
            methods[method]=scopes
        result[split]={'meetings':len(selected)//len(METHODS),'methods':methods}
    return result


def experiment(a):
    start=time.perf_counter();cpu=time.process_time()
    if digest(a.geometry/'results.json')!=GEOMETRY:raise ValueError('immutable block geometry required')
    old=json.loads((a.geometry/'results.json').read_text());data=read(a)
    if old['source_sha256']!=data.source or old['support']['cancelled_blocks']!=0 or len(old['records'])!=162:
        raise ValueError('matched complete source required')
    for name,pin in old['code_pins'].items():
        if digest(Path(__file__).with_name(name))!=pin:raise ValueError('original geometry code drift')
    if digest(a.geometry/'block-directions.npy')!=old['files']['block-directions.npy']['sha256']:raise ValueError('direction drift')
    directions=np.load(a.geometry/'block-directions.npy',mmap_mode='r',allow_pickle=False)
    setup=(time.perf_counter()-start)*1000;records=[];timings=[];support=Counter()
    with threadpool_limits(limits=1):
        for meeting,original in zip(data.meetings,old['records']):
            if meeting.ordinal!=original['meeting']:raise ValueError('complete original order required')
            offset=original['source_offset'];size=len(meeting.turns);blocks=original['blocks'];owners=np.full(size,-1,dtype=np.int64)
            for b in blocks:owners[b['start']:b['end']+1]=b['ordinal']
            if not np.array_equal(owners>=0,data.ready[offset:offset+size]):raise ValueError('ready lineage drift')
            selected=np.flatnonzero(owners>=0).tolist();gold,parts=components(meeting,selected);owned=owners[selected].tolist()
            lo=original['block_directions_offset'];dense=directions[lo:lo+len(blocks)]
            begin=time.perf_counter();counts=block_counts(data.counts[offset:offset+size],owners,len(blocks));lex=lexical(counts,data.idf)
            features_ms=(time.perf_counter()-begin)*1000;support.update(sources=size,ready=len(selected),known=sum(g is not None for g in gold),
                blocks=len(blocks),zero_lexical_blocks=int((np.asarray(lex.getnnz(axis=1))==0).sum()),native_topics=len(meeting.topics))
            for method in METHODS:
                begin=time.perf_counter();merges=None
                if method=='lexical_average':tree,merges=average((lex@lex.T).toarray())
                elif method=='dense_average':tree,merges=average(dense@dense.T)
                else:tree=temporal(len(blocks),method)
                fit_ms=(time.perf_counter()-begin)*1000;begin=time.perf_counter()
                metrics=evaluate(tree,owned,gold,parts,selected,len(meeting.topics))
                timings.append({'meeting':meeting.ordinal,'method':method,'features_ms':features_ms if method==METHODS[0] else 0.,
                    'fit_ms':fit_ms,'evaluation_ms':(time.perf_counter()-begin)*1000})
                records.append({'meeting':meeting.ordinal,'split':original['split'],'method':method,'tree':stored_tree(tree),
                    'merges':merges,'metrics':metrics})
            if meeting.ordinal%20==0:print(json.dumps({'processed_meetings':meeting.ordinal+1}),flush=True)
    if support['sources']!=88336 or support['ready']!=84434 or support['known']!=77892 or support['blocks']!=8196:
        raise ValueError('complete source support drift')
    summary=summarize(records)
    if any(summary['all']['methods'][m]['crossspan']['pairs']!=84323 for m in METHODS):raise ValueError('audited distinct-block return support drift')
    return {'schema_version':1,'scope':'complete_training_offline_cut_free_block_hierarchy_not_selection',
        'source_sha256':data.source,'source_capture_sha256':data.capture,'geometry_sha256':GEOMETRY,
        'block_file_sha256':old['files']['block-directions.npy']['sha256'],'support':dict(support),
        'protocol':{'blocks':'immutable_original16turn512token_gap_policy','lexical':'original_full_training_vocabulary_IDF_sublinearTF_L2_no_projection',
            'dense':'unchanged_equal_source_direction_pool_L2','distance':'clipped1_minus_cosine_zero_diagonal_zero_lexical_retained',
            'discovery':'SciPy_average_linkage_original_block_order_ties_no_cut',
            'metric':'singleton_known_source_weighted_class_fraction_at_physical_block_LCA',
            'all_scopes_exclude':'same_physical_block_source_pairs','far':'original_source_gap_greater_than4',
            'crossspan':'far_and_distinct_merged_native_topic_components','unknown_multi':'retained_geometry_excluded_conditional_gold',
            'baselines':['root_only',*Baseline],'partition':'explored_internal54_reserved108_development_not_pristine',
            'new_inference':0,'llm_calls':0,'held_out_sources_read':False,'frontier_selection':False,
            'online_method':False,'semantic_ancestors':False,'full_ingestion_gate':False},
        'dependency':{'numpy':np.__version__,'scipy':scipy.__version__,'linkage_module_sha256':digest(Path(linkage.__code__.co_filename))},
        'summary':summary,'records':records,'code_pins':{n:digest(Path(__file__).with_name(n)) for n in
            ['qmsum_block_hierarchy.py','block_hierarchy.py','captured_hierarchy.py','qmsum_hierarchy.py','qmsum_unique_blocks.py',
             'episode_blocks.py','discovery_inputs.py','qmsum.py','uv.lock']},
        'timings':{'setup_ms':setup,'after_imports_ms':(time.perf_counter()-start)*1000,'cpu_ms':(time.process_time()-cpu)*1000,
                   'peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,'stages':timings}}


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for key in ['source','vectors','units','labels','baseline','reference-manifest','geometry','output']:p.add_argument('--'+key,type=Path,required=True)
    p.add_argument('--compare',type=Path);a=p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    result=experiment(a)
    if a.compare:
        old=json.loads(a.compare.read_text())
        if json.dumps({k:v for k,v in old.items() if k!='timings'},sort_keys=True,allow_nan=False)!=json.dumps({k:v for k,v in result.items() if k!='timings'},sort_keys=True,allow_nan=False):
            raise ValueError('complete hierarchy repeat differs')
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps({'support':result['support'],'summary':result['summary']['all'],'timings':{k:v for k,v in result['timings'].items() if k!='stages'}}),flush=True)


if __name__=='__main__':main()
