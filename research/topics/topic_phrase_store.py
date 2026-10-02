"""Bounded mutable source ledger for deferred phrase snapshots; no inference."""
from dataclasses import asdict
from topic_phrases import PhraseSource,discover


class PhraseStore:
    def __init__(self,namespace):
        if not isinstance(namespace,str) or not namespace or len(namespace)>4096:raise ValueError('explicit topic namespace required')
        self.namespace=namespace;self.records={};self.versions={};self.revision=0;self.text_bytes=0

    def upsert(self,source):
        if (not isinstance(source,PhraseSource) or any(not isinstance(v,str) or not v or len(v)>4096 for v in [source.id,source.thread,source.provider])
                or not isinstance(source.text,str) or len(source.text.encode())>262144):raise ValueError('bounded typed source required')
        prior=self.records.get(source.id)
        if prior==source:return False
        if prior is None and len(self.records)>=2048:raise ValueError('source capacity exceeded')
        delta=len(source.text.encode())-(len(prior.text.encode()) if prior else 0)
        if self.text_bytes+delta>8*1024*1024:raise ValueError('text capacity exceeded')
        if any(row.id!=source.id and row.thread==source.thread and row.provider!=source.provider for row in self.records.values()):
            raise ValueError('thread provider scope differs')
        self.revision+=1;self.records[source.id]=source;self.versions[source.id]=self.revision;self.text_bytes+=delta
        return True

    def delete(self,identity):
        source=self.records.pop(identity,None)
        if source is None:return False
        self.text_bytes-=len(source.text.encode());del self.versions[identity];self.revision+=1;return True

    def snapshot(self):
        result=discover([self.records[key] for key in sorted(self.records)],self.namespace)
        return {**result,'revision':self.revision,'versions':dict(sorted(self.versions.items()))}

    def checkpoint(self):
        return {'schema_version':1,'namespace':self.namespace,'revision':self.revision,
            'records':[{'source':asdict(self.records[key]),'version':self.versions[key]} for key in sorted(self.records)]}

    @classmethod
    def restore(cls,value):
        if not isinstance(value,dict) or set(value)!={'schema_version','namespace','revision','records'} or type(value['schema_version']) is not int or value['schema_version']!=1:
            raise ValueError('known checkpoint schema required')
        revision=value['revision'];rows=value['records']
        if type(revision) is not int or not 0<=revision<=2**53 or not isinstance(rows,list) or len(rows)>2048:raise ValueError('bounded counters and source ledger required')
        model=cls(value['namespace']);versions=set()
        for row in rows:
            if (not isinstance(row,dict) or set(row)!={'source','version'} or not isinstance(row['source'],dict)
                    or set(row['source'])!={'id','thread','provider','text'} or type(row['version']) is not int
                    or not 1<=row['version']<=revision or row['version'] in versions):raise ValueError('unique valid source versions required')
            source=PhraseSource(**row['source'])
            if source.id in model.records:raise ValueError('duplicate source identity')
            model.upsert(source);model.versions[source.id]=row['version'];versions.add(row['version'])
        model.revision=revision;return model
