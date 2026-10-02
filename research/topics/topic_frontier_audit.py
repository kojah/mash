"""Scalar source-vector check of frontier artifacts, without producer imports."""
import argparse
import json
import math
from pathlib import Path


def audit(baseline,first,repeat):
    def stable(result):
        result.pop('runtime_ms')
        for arm in result['arms'].values():arm.pop('runtime_ms')
        return result
    a=stable(json.loads(first.read_text()));b=stable(json.loads(repeat.read_text()))
    assert a==b
    report=json.loads(baseline.read_text())
    vectors={r['id']:r['vector'] for r in report['baseline_checkpoint']['records']}
    checked=0;maximum=0.;margin=1.
    for name,arm in a['arms'].items():
        for direction in ['forward','reverse']:
            nodes={n['id']:n for n in report[direction]['nodes']};out=arm[direction];eligible={}
            for identity,node in nodes.items():
                points=[vectors[s] for s in node['sources']];scores=[]
                if len(points)>1:
                    for i,point in enumerate(points):
                        other=[math.fsum(q[j] for k,q in enumerate(points) if k!=i) for j in range(len(point))]
                        norm=math.sqrt(math.fsum(x*x for x in other))
                        scores.append(math.fsum(x*y/norm for x,y in zip(point,other)) if norm>1e-12 else 0.)
                value=min(scores) if scores else None
                stored=out['node_checks'][identity]['minimum_leave_one_out_cosine']
                assert (value is None)==(stored is None)
                if value is not None:
                    maximum=max(maximum,abs(value-stored));margin=min(margin,abs(value-.55))
                    assert abs(value-stored)<1e-12
                ok=len(node['labels'])>=2 and len(points)>=2 and (name=='label_support_only' or value>=.55)
                assert ok==out['node_checks'][identity]['eligible']
                eligible[identity]=ok;checked+=1
            expected=[]
            for identity in nodes:
                if not eligible[identity]:continue
                descendants=set();stack=list(nodes[identity]['children'])
                while stack:
                    child=stack.pop();descendants.add(child);stack.extend(nodes[child]['children'])
                if not any(eligible[d] for d in descendants):expected.append(identity)
            assert sorted(expected)==out['selected']
            for sid,tags in out['memberships'].items():
                assert sorted(tags)==sorted(n for n in expected if sid in nodes[n]['sources'])
    return {'stable_repeat':True,'node_checks':checked,'maximum_scalar_score_difference':maximum,
            'minimum_threshold_margin':margin,
            'scope':'independent math.fsum cohesion, eligibility, deepest frontier, memberships; not semantic quality or full routing parity'}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['baseline','first','repeat']:parser.add_argument('--'+name,type=Path,required=True)
    args=parser.parse_args()
    print(json.dumps(audit(args.baseline,args.first,args.repeat),sort_keys=True))
