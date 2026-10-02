"""Exploratory supported/cohesive cuts of an existing deferred tree; no refitting."""
import argparse
import hashlib
import json
from pathlib import Path
import time
import numpy as np


def frontier(snapshot,checkpoint,cohesion=True):
    nodes={node['id']:node for node in snapshot['nodes']}
    records={r['id']:r for r in checkpoint['records']}
    threshold=snapshot['parameters']['threshold']
    diagnostics={}; selected=[]
    for identity,node in nodes.items():
        vectors=[records[sid]['vector'] for sid in node['sources']]
        if any(v is None for v in vectors):raise ValueError('tree source lacks stored geometry')
        points=np.asarray(vectors,dtype=float)
        scores=[]
        if len(points)>=2:
            total=points.sum(axis=0)
            for point in points:
                other=total-point; norm=float(np.linalg.norm(other))
                scores.append(float(point@(other/norm)) if norm>1e-12 else 0.)
        supported=len(node['labels'])>=2 and len(vectors)>=2
        geometric=bool(scores) and min(scores)>=threshold
        diagnostics[identity]={'label_supported':supported,'minimum_leave_one_out_cosine':min(scores) if scores else None,
                               'eligible':supported and (geometric or not cohesion)}
    def visit(identity):
        children=nodes[identity]['children']
        below=[]
        for child in children:below.extend(visit(child))
        # Prefer the deepest eligible nodes. Unsupported residual branches stay
        # unassigned: do not hide lost coverage by assigning their parent too.
        if below:return below
        return [identity] if diagnostics[identity]['eligible'] else []
    if snapshot['root']:selected=sorted(visit(snapshot['root']))
    memberships={sid:[] for sid in records}
    for identity in selected:
        for sid in nodes[identity]['sources']:memberships[sid].append(identity)
    threads=[]
    for row in snapshot['threads']:
        direct=sorted({topic for sid in row['sources'] for topic in memberships[sid]})
        ancestors=set()
        for identity in direct:
            parent=nodes[identity]['parent']
            while parent is not None:ancestors.add(parent);parent=nodes[parent]['parent']
        threads.append({'thread':row['thread'],'direct_tags':direct,'ancestor_tags':sorted(ancestors)})
    ready={sid for sid,r in records.items() if r['vector'] is not None}
    covered={sid for sid,tags in memberships.items() if tags}
    return {'selected':selected,'memberships':memberships,'threads':threads,'node_checks':diagnostics,
            'summary':{'topics':len(selected),'embedded_sources':len(ready),'covered_sources':len(covered),
                       'coverage':len(covered)/len(ready) if ready else None,
                       'multi_topic_sources':sum(len(tags)>1 for tags in memberships.values()),
                       'maximum_sources_per_topic':max((len(nodes[i]['sources']) for i in selected),default=0)}}


def pairs(result):
    import itertools
    groups={topic:[] for topic in result['selected']}
    for sid,tags in result['memberships'].items():
        for topic in tags:groups[topic].append(sid)
    return {tuple(pair) for members in groups.values() for pair in itertools.combinations(sorted(members),2)}


def experiment(directory):
    report=json.loads((directory/'results.json').read_text());checkpoint=report['baseline_checkpoint']
    arms={}; start=time.perf_counter()
    for name,cohesion in [('label_support_only',False),('label_support_and_cohesion',True)]:
        begin=time.perf_counter();forward=frontier(report['forward'],checkpoint,cohesion)
        reverse=frontier(report['reverse'],checkpoint,cohesion)
        left=pairs(forward);right=pairs(reverse)
        covered_left={sid for sid,tags in forward['memberships'].items() if tags}
        covered_right={sid for sid,tags in reverse['memberships'].items() if tags}
        arms[name]={'forward':forward,'reverse':reverse,
                    'order':{'pair_jaccard':len(left&right)/len(left|right) if left|right else None,
                             'coverage_jaccard':len(covered_left&covered_right)/len(covered_left|covered_right) if covered_left|covered_right else None,
                             'forward_pairs':len(left),'reverse_pairs':len(right)},
                    'runtime_ms':(time.perf_counter()-begin)*1000}
    return {'schema_version':1,'baseline_sha256':hashlib.sha256((directory/'results.json').read_bytes()).hexdigest(),
            'method_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'arms':arms,
            'policy':{'minimum_label_terms':2,'minimum_sources':2,'cohesion_threshold':report['forward']['parameters']['threshold'],
                      'selection':'deepest eligible antichain; uncovered residuals abstain','new_inference':0},
            'semantic_quality_verified':False,'production_selected':False,'runtime_ms':(time.perf_counter()-start)*1000}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args();result=experiment(args.baseline)
    with args.output.open('x') as file:json.dump(result,file,sort_keys=True,allow_nan=False)
    args.output.chmod(0o600)
    print(json.dumps({name:{'forward':arm['forward']['summary'],'reverse':arm['reverse']['summary'],'order':arm['order']}
                      for name,arm in result['arms'].items()},sort_keys=True))
