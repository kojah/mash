"""Source/LCA/height audit plus independent global-minimum linkage decisions."""
import argparse
import json
from pathlib import Path
import numpy as np
from audit_unit_granularity import audit,sha
from linkage_minimum import verify


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--artifacts',type=Path,required=True);p.add_argument('--capture',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    source_pins={n:sha(Path(__file__).with_name(n)) for n in ['audit_unit_granularity_global.py','audit_unit_granularity.py','linkage_minimum.py']}
    source=audit(a.artifacts,a.capture);data=json.loads(a.capture.read_text());gp=a.artifacts/'topic-qmsum-block-geometry-first-20261002/results.json'
    geometry=json.loads(gp.read_text());vr=a.artifacts/'topic-qmsum-semantic-vectors-first-20261002';vm=json.loads((vr/'results.json').read_text())
    assert sha(vr/'raw-vectors.npy')==vm['files']['raw-vectors.npy']['sha256'];raw=np.load(vr/'raw-vectors.npy',mmap_mode='r',allow_pickle=False)
    pp=gp.parent/'block-directions.npy';assert sha(pp)==data['block_vectors_sha256'];pooled=np.load(pp,mmap_mode='r',allow_pickle=False)
    checks=[]
    for row in data['records']:
        ordinal=row['meeting'];g=geometry['records'][ordinal];offset=g['source_offset'];positions=row['source_ordinals'];name=row['representation']
        if name=='physical_block':v=pooled[g['block_directions_offset']:g['block_directions_offset']+len(g['blocks'])]
        elif name=='raw_message':v=raw[offset+np.array(positions)]
        else:
            v=[]
            for i in positions:
                start=data['context_lineage'][ordinal]['units'][i]['start'];summed=np.sum(raw[offset+start:offset+i+1],axis=0,dtype=np.float64);v.append(summed/np.linalg.norm(summed))
            v=np.array(v)
        distance=1-np.clip(v@v.T,-1,1);np.fill_diagonal(distance,0)
        checks.append({'meeting':ordinal,'representation':name,**verify(distance,row['merges'])})
        if name=='physical_block' and ordinal%20==0:print(json.dumps({'globally_audited_meetings':ordinal+1}),flush=True)
    assert source_pins=={n:sha(Path(__file__).with_name(n)) for n in source_pins};assert len(checks)==486
    result={'capture_sha256':sha(a.capture),'code_pins':source_pins,'source_audit':source,'global_minimum_checks':checks,
        'maximum_chosen_minus_global_minimum':max(c['maximum_chosen_minus_global_minimum'] for c in checks),
        'limitations':['global minima and mean heights verified within frozen1e-12; does not require identical tie choices',
            'same source-audit frozen-vector/NumPy limitations; not label/semantic/online/full-ingestion/Rust parity evidence']}
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps({'trees':len(checks),'merges':sum(c['merges'] for c in checks),'maximum_gap':result['maximum_chosen_minus_global_minimum']}),flush=True)


if __name__=='__main__':main()
