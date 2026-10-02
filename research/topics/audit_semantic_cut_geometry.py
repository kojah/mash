"""Independent direct-pair audit of frozen cuts, not a full label/pair oracle."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import numpy as np


def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()


def audit(root,path):
    data=json.loads(path.read_text());geometry_path=root/'topic-qmsum-block-geometry-first-20261002/results.json'
    assert sha(geometry_path)==data['geometry_sha256'];geometry=json.loads(geometry_path.read_text())
    vectors_path=geometry_path.parent/'block-directions.npy';assert sha(vectors_path)==data['vectors_sha256']
    vectors=np.load(vectors_path,mmap_mode='r',allow_pickle=False)
    controls={}
    for frame,p in [('fixed_tf',root/'topic-qmsum-stable-frame-quality-first-20261002.json'),
                    ('calibrated_32',root/'topic-qmsum-calibration-final-first-20261002/results.json')]:
        assert sha(p)==data['control_sha256'][frame]
        controls[frame]={(r['meeting'],r['order']):r for r in json.loads(p.read_text())['records'] if r['policy']=='static_noise_filtered_tf' and r['kind']=='hashed'}
    for n,pin in data['code_pins'].items():assert sha(Path(__file__).with_name(n))==pin
    nodes=trees=0;maximum=0.
    def close(a,b):
        nonlocal maximum
        maximum=max(maximum,abs(a-b));assert math.isclose(a,b,rel_tol=0,abs_tol=1e-12),(a,b)
    for row in data['records']:
        if row['semantic_cut'] is None:continue
        g=geometry['records'][row['meeting']];n=len(g['blocks']);lo=g['block_directions_offset'];v=vectors[lo:lo+n]
        tree=controls[row['frame']][row['meeting'],row['order']]['tree'];children={int(k):x for k,x in tree['children'].items()};leaves={int(k):x for k,x in tree['leaves'].items()}
        gram=v@v.T;upper=gram[np.triu_indices(n,1)];mean=math.fsum(upper)/len(upper) if len(upper) else 0.
        sd=math.sqrt(math.fsum((float(x)-mean)**2 for x in upper)/len(upper)) if len(upper) else 0.;threshold=mean+sd
        c=row['semantic_cut'];close(mean,c['mean']);close(sd,c['population_sd']);close(threshold,c['threshold'])
        captured={x['serial']:x for x in c['nodes']};members={};scores={};seen=set()
        def visit(node):
            nonlocal nodes
            assert node not in seen;seen.add(node)
            m=[leaves[node]] if node in leaves else sorted(visit(children[node][0])+visit(children[node][1]))
            score=math.fsum(float(gram[a,b]) for pos,a in enumerate(m) for b in m[pos+1:])/(len(m)*(len(m)-1)/2) if len(m)>1 else None
            entry=captured[node];assert m==entry['blocks']
            if score is None:assert entry['distinct_pair_mean'] is None
            else:close(score,entry['distinct_pair_mean'])
            qualifies=score is not None and score>threshold+1e-12
            assert qualifies==entry['qualifies'];members[node]=m;scores[node]=qualifies;nodes+=1
            return m
        assert visit(tree['root'])==list(range(n));assert seen==set(captured)==set(children)|set(leaves)
        groups=[];unassigned=[]
        def cut(node):
            if scores[node]:groups.append(members[node])
            elif node in leaves:unassigned.append(leaves[node])
            else:
                for child in children[node]:cut(child)
        cut(tree['root']);assert sorted(groups)==c['groups'];assert sorted(unassigned)==c['unassigned']
        assert [p['blocks'] for p in row['proposals']]==sorted(groups);trees+=1
    assert trees==648
    return {'capture_sha256':sha(path),'auditor_sha256':sha(Path(__file__)),'trees':trees,'nodes':nodes,'maximum_pair_mean_error':maximum,
        'scope':'independent_direct_pair_geometry_and_cut_decisions_only','limitations':['shared frozen vectors and NumPy dot products',
            'does not audit label counts/rankings, source pair metrics, representatives, summaries or original tree construction',
            'not semantic quality, production ingestion, lifecycle or Rust parity evidence']}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--artifacts',type=Path,required=True);p.add_argument('--capture',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    result=audit(a.artifacts,a.capture)
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps(result),flush=True)


if __name__=='__main__':main()
