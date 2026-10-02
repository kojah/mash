"""Bounded cosine routing with coherent deferred snapshots; research only."""
from dataclasses import dataclass
from enum import StrEnum
import math
import numpy as np


class RouteState(StrEnum):
    ASSIGNED = 'assigned'
    MISSING = 'missing_embedding'
    CAPACITY = 'leaf_capacity'


@dataclass(frozen=True)
class Source:
    id: str
    thread: str
    provider: str
    text: str
    vector: np.ndarray | None


@dataclass(frozen=True)
class Record:
    source: Source
    version: int
    anchor: int | None
    state: RouteState


@dataclass
class Leaf:
    id: int
    sum: np.ndarray
    count: int


@dataclass(frozen=True)
class Capture:
    revision: int
    records: tuple[Record, ...]
    leaf_ids: tuple[int, ...]
    centers: np.ndarray
    sums: np.ndarray
    counts: tuple[int, ...]


class Stream:
    def __init__(self,dimensions=384,maximum_leaves=128,maximum_sources=2048,maximum_text_bytes=8*1024*1024,threshold=.55):
        if (type(dimensions) is not int or not 1 <= dimensions <= 1024 or type(maximum_leaves) is not int
                or not 1 <= maximum_leaves <= 2000 or type(maximum_sources) is not int or not 1 <= maximum_sources <= 2048
                or type(maximum_text_bytes) is not int or not 1 <= maximum_text_bytes <= 8*1024*1024
                or isinstance(threshold,bool) or not math.isfinite(threshold) or not 0 < threshold <= 1):
            raise ValueError('bounded explicit stream configuration required')
        self.dimensions = dimensions; self.maximum_leaves = maximum_leaves; self.maximum_sources = maximum_sources; self.threshold = threshold
        self.maximum_text_bytes = maximum_text_bytes; self.text_bytes = 0
        self.records = {}; self.leaves = {}; self.next_leaf = 0; self.revision = 0; self.snapshot = None

    def source(self,value):
        if not isinstance(value,Source) or any(not isinstance(v,str) or not v or len(v) > 4096 for v in [value.id,value.thread,value.provider]):
            raise ValueError('source identity and scope required')
        if not isinstance(value.text,str) or len(value.text.encode()) > 262144: raise ValueError('bounded represented source text required')
        vector = None
        if value.vector is not None:
            original = np.asarray(value.vector)
            if original.dtype.kind not in 'fiu': raise ValueError('numeric stored vector required')
            vector = np.array(original,dtype=np.float64,copy=True)
            if vector.shape != (self.dimensions,) or not np.isfinite(vector).all() or abs(float(np.linalg.norm(vector))-1) > 1e-12:
                raise ValueError('compatible finite unit direction required')
            vector.flags.writeable = False
        return Source(value.id,value.thread,value.provider,value.text,vector)

    def remove(self,record):
        if record.anchor is not None:
            leaf = self.leaves[record.anchor]; leaf.sum -= record.source.vector; leaf.count -= 1
            if not leaf.count: del self.leaves[leaf.id]

    def upsert(self,value):
        source = self.source(value); old = self.records.get(source.id)
        if old is not None:
            same_vector = (old.source.vector is None and source.vector is None) or (old.source.vector is not None and source.vector is not None and np.array_equal(old.source.vector,source.vector))
            if same_vector and (old.source.thread,old.source.provider,old.source.text) == (source.thread,source.provider,source.text): return old
        elif len(self.records) == self.maximum_sources: raise ValueError('source budget exceeded without dropping records')
        size = len(source.text.encode())-(len(old.source.text.encode()) if old is not None else 0)
        if self.text_bytes+size > self.maximum_text_bytes: raise ValueError('text budget exceeded without dropping records')
        # Validation precedes removing the previous version. All model mutation
        # stays on the caller thread; background builders only receive captures.
        if old is not None:
            self.remove(old); self.snapshot = None
        anchor = None; state = RouteState.MISSING
        if source.vector is not None:
            candidates = []
            for identity,leaf in sorted(self.leaves.items()):
                norm = float(np.linalg.norm(leaf.sum))
                if norm > 1e-12: candidates.append((float((leaf.sum/norm)@source.vector),identity))
            candidates.sort(key=lambda v:(-v[0],v[1]))
            if candidates and candidates[0][0] >= self.threshold: anchor = candidates[0][1]
            elif len(self.leaves) < self.maximum_leaves:
                anchor = self.next_leaf; self.next_leaf += 1; self.leaves[anchor] = Leaf(anchor,np.zeros(self.dimensions),0)
            if anchor is not None:
                leaf = self.leaves[anchor]; leaf.sum += source.vector; leaf.count += 1; state = RouteState.ASSIGNED
            else: state = RouteState.CAPACITY
        self.revision += 1; self.text_bytes += size; record = Record(source,self.revision,anchor,state); self.records[source.id] = record
        return record

    def delete(self,identity):
        old = self.records.pop(identity,None)
        if old is None: return False
        self.remove(old); self.text_bytes -= len(old.source.text.encode()); self.revision += 1; self.snapshot = None; return True

    def capture(self):
        ids = []; centers = []; sums = []; counts = []
        for identity,leaf in sorted(self.leaves.items()):
            norm = float(np.linalg.norm(leaf.sum))
            if norm > 1e-12:
                ids.append(identity); centers.append(leaf.sum/norm); sums.append(leaf.sum.copy()); counts.append(leaf.count)
        matrix = np.array(centers,dtype=np.float64).reshape((-1,self.dimensions)); matrix.flags.writeable = False
        raw = np.array(sums,dtype=np.float64).reshape((-1,self.dimensions)); raw.flags.writeable = False
        return Capture(self.revision,tuple(self.records[k] for k in sorted(self.records)),tuple(ids),matrix,raw,tuple(counts))

    def publish(self,snapshot):
        # Insert-only lag is explicit and safe. An edit/delete of any captured
        # source rejects publication; old evidence never survives that mutation.
        if type(snapshot['revision']) is not int or not 0 <= snapshot['revision'] <= self.revision:
            raise ValueError('captured source revision required')
        if (self.snapshot is not None and snapshot['revision'] < self.snapshot['revision']) or any(
                identity not in self.records or self.records[identity].version != version for identity,version in snapshot['versions'].items()):
            return False
        self.snapshot = snapshot; return True

    def freshness(self):
        return {'source_revision':self.revision,'topic_revision':self.snapshot['revision'] if self.snapshot is not None else None,
            'pending_refresh':self.snapshot is None or self.snapshot['revision'] != self.revision,
            'unrepresented_sources':len(self.records)-(len(self.snapshot['versions']) if self.snapshot is not None else 0)}

    def checkpoint(self):
        return {'schema_version':1,'configuration':{'dimensions':self.dimensions,'maximum_leaves':self.maximum_leaves,'maximum_sources':self.maximum_sources,'maximum_text_bytes':self.maximum_text_bytes,'threshold':self.threshold},
            'revision':self.revision,'next_leaf':self.next_leaf,
            'records':[{'id':r.source.id,'thread':r.source.thread,'provider':r.source.provider,'text':r.source.text,
                'vector':None if r.source.vector is None else r.source.vector.tolist(),'version':r.version,'anchor':r.anchor,'state':r.state}
                for r in self.capture().records],
            'leaves':[{'id':i,'sum':v.sum.tolist(),'count':v.count} for i,v in sorted(self.leaves.items())]}

    @classmethod
    def restore(cls,value):
        if not isinstance(value,dict) or set(value) != {'schema_version','configuration','revision','next_leaf','records','leaves'} or type(value['schema_version']) is not int or value['schema_version'] != 1:
            raise ValueError('known checkpoint schema required')
        if not isinstance(value['configuration'],dict) or set(value['configuration']) != {'dimensions','maximum_leaves','maximum_sources','maximum_text_bytes','threshold'}: raise ValueError('known stream configuration required')
        model = cls(**value['configuration'])
        for field in ['revision','next_leaf']:
            if type(value[field]) is not int or not 0 <= value[field] <= 2**53: raise ValueError('bounded checkpoint counters required')
        model.revision = value['revision']; model.next_leaf = value['next_leaf']
        if model.next_leaf > model.revision: raise ValueError('checkpoint counter sequence differs')
        if not isinstance(value['records'],list) or len(value['records']) > model.maximum_sources or not isinstance(value['leaves'],list) or len(value['leaves']) > model.maximum_leaves:
            raise ValueError('bounded checkpoint ownership required')
        for row in value['leaves']:
            if set(row) != {'id','sum','count'} or type(row['id']) is not int or not 0 <= row['id'] < model.next_leaf or row['id'] in model.leaves or type(row['count']) is not int or not 1 <= row['count'] <= model.maximum_sources:
                raise ValueError('unique live leaf identities/counts required')
            vector = np.array(row['sum'],dtype=float)
            if vector.shape != (model.dimensions,) or not np.isfinite(vector).all(): raise ValueError('finite centroid sum required')
            model.leaves[row['id']] = Leaf(row['id'],vector,row['count'])
        replay = {i:[] for i in model.leaves}; versions = set()
        for row in value['records']:
            if set(row) != {'id','thread','provider','text','vector','version','anchor','state'}: raise ValueError('known source record schema required')
            source = model.source(Source(row['id'],row['thread'],row['provider'],row['text'],None if row['vector'] is None else np.array(row['vector'])))
            anchor = row['anchor']; state = RouteState(row['state']); version = row['version']
            if source.id in model.records or type(version) is not int or not 1 <= version <= model.revision or version in versions or (anchor is not None and (type(anchor) is not int or anchor not in replay)):
                raise ValueError('unique valid source versions/ownership required')
            if (state == RouteState.ASSIGNED) != (anchor is not None) or (state == RouteState.MISSING) != (source.vector is None): raise ValueError('source state/geometry mismatch')
            versions.add(version); model.text_bytes += len(source.text.encode())
            if model.text_bytes > model.maximum_text_bytes: raise ValueError('checkpoint text budget exceeded')
            model.records[source.id] = Record(source,version,anchor,state)
            if anchor is not None: replay[anchor].append(source.vector)
        for identity,vectors in replay.items():
            leaf = model.leaves[identity]
            if len(vectors) != leaf.count or not np.allclose(np.sum(vectors,axis=0),leaf.sum,atol=1e-10,rtol=0): raise ValueError('checkpoint centroid/ownership differs')
        return model
