"""Causal adjacent user-turn pooling for routing, retaining original label evidence."""
import argparse
from enum import StrEnum
import hashlib
import json
from pathlib import Path
import time
import numpy as np
from topic_stream import Source,Stream,Capture,Record,RouteState
from topic_refresh import refresh
from topic_stream_demo import pair_members


class Adjacency(StrEnum):
    CANONICAL = 'canonical_break_at_exclusions'
    ADMITTED = 'admitted_text_with_exclusions_separate'


def units(sources,threshold=.55,maximum_turns=8,maximum_joined_bytes=8192,adjacency=Adjacency.CANONICAL):
    if not 0<threshold<=1 or type(maximum_turns) is not int or not 1<=maximum_turns<=8:
        raise ValueError('bounded explicit unit parameters required')
    if type(maximum_joined_bytes) is not int or not 1<=maximum_joined_bytes<=8192:
        raise ValueError('bounded represented unit text required')
    if not isinstance(adjacency,Adjacency):raise ValueError('known typed adjacency policy required')
    grouped={}
    for source in sources:
        rows=grouped.setdefault(source.thread,[])
        if rows and rows[0].provider!=source.provider:raise ValueError('thread provider changes')
        rows.append(source)
    result={}
    for thread,rows in grouped.items():
        completed=[];pending=[];total=None;size=0
        for source in rows:
            if adjacency is Adjacency.ADMITTED and source.vector is None and source.text=='':
                # Only an explicit empty projection is an exclusion. Missing
                # geometry for nonempty text remains a boundary in both arms.
                completed.append((source,));continue
            length=len(source.text.encode())
            accept=False
            if pending and source.vector is not None and total is not None:
                norm=float(np.linalg.norm(total))
                accept=(len(pending)<maximum_turns and size+length+2<=maximum_joined_bytes
                        and norm>1e-12 and float(source.vector@(total/norm))>=threshold)
            if pending and not accept:completed.append(tuple(pending));pending=[];total=None;size=0
            if source.vector is None:
                completed.append((source,));continue
            pending.append(source);total=source.vector.copy() if total is None else total+source.vector
            size+=length+(2 if len(pending)>1 else 0)
        if pending:completed.append(tuple(pending))
        result[thread]=completed
    # Native per-thread order is supplied by the caller. Round-robin unit
    # arrival is simulated; it is not reconstructed network arrival.
    return [result[thread][i] for i in range(max(map(len,result.values()),default=0))
            for thread in sorted(result) if i<len(result[thread])]


def route(groups):
    model=Stream();anchors={};original=[]
    for group in groups:
        ids=[s.id for s in group]
        identity='unit-'+hashlib.sha256(json.dumps(ids,separators=(',',':')).encode()).hexdigest()[:24]
        total=sum((s.vector for s in group if s.vector is not None),np.zeros(model.dimensions))
        norm=float(np.linalg.norm(total));vector=total/norm if norm>1e-12 else None
        record=model.upsert(Source(identity,group[0].thread,group[0].provider,'\n\n'.join(s.text for s in group),vector))
        for source in group:anchors[source.id]=record.anchor;original.append(source)
    records=tuple(Record(s,i+1,anchors[s.id],RouteState.MISSING if s.vector is None else
                         RouteState.ASSIGNED if anchors[s.id] is not None else RouteState.CAPACITY)
                  for i,s in enumerate(sorted(original,key=lambda s:s.id)))
    leaves=sorted({r.anchor for r in records if r.anchor is not None});sums=[];centers=[];counts=[];active=[]
    for identity in leaves:
        members=[r.source.vector for r in records if r.anchor==identity]
        total=np.sum(members,axis=0);norm=float(np.linalg.norm(total))
        if norm<=1e-12:continue
        active.append(identity);sums.append(total);centers.append(total/norm);counts.append(len(members))
    # Refresh uses ORIGINAL vectors/text and original-source support, avoiding
    # artificial duplicate pooled vectors or a change in DF denominator.
    capture=Capture(len(records),records,tuple(active),np.asarray(centers).reshape((-1,384)),
                    np.asarray(sums).reshape((-1,384)),tuple(counts))
    return refresh(capture)


def experiment(baseline):
    report=json.loads(baseline.read_text());records=report['baseline_checkpoint']['records']
    order={p['id']:(p['thread'],p['ordinal'],p['original_message']) for p in report['source_provenance']}
    sources=[Source(r['id'],r['thread'],r['provider'],r['text'],None if r['vector'] is None else np.array(r['vector']))
             for r in sorted(records,key=lambda r:order[r['id']])]
    start=time.perf_counter();groups=units(sources);forward=route(groups);reverse=route(list(reversed(groups)))
    left=pair_members(forward);right=pair_members(reverse)
    return {'schema_version':1,'baseline_sha256':hashlib.sha256(baseline.read_bytes()).hexdigest(),
            'method_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
            'dependency_sha256':{name:hashlib.sha256(Path(__file__).with_name(name).read_bytes()).hexdigest()
                                 for name in ['topic_stream.py','topic_refresh.py','topic_stream_demo.py']},
            'policy':{'threshold':.55,'maximum_turns':8,'maximum_joined_bytes':8192,'new_inference':0,
                      'geometry':'normalized mean of existing unit vectors for routing; original vectors for deferred memberships',
                      'arrival':'round-robin native-order units; reverse unit arrival retains unit formation'},
            'units':[[s.id for s in group] for group in groups],'forward':forward,'reverse':reverse,
            'summary':{'original_sources':len(sources),'units':len(groups),'multi_turn_units':sum(len(g)>1 for g in groups),
                       'maximum_turns_observed':max(map(len,groups),default=0),
                       'pair_jaccard':len(left&right)/len(left|right) if left|right else None},
            'production_selected':False,'semantic_quality_verified':False,'runtime_ms':(time.perf_counter()-start)*1000}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args();result=experiment(args.baseline)
    with args.output.open('x') as file:json.dump(result,file,sort_keys=True,allow_nan=False)
    args.output.chmod(0o600)
    print(json.dumps({'summary':result['summary'],'forward':result['forward']['diagnostics'],'reverse':result['reverse']['diagnostics']}))
