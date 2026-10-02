"""One deferred worker and one coalesced request, never a snapshot backlog."""
from concurrent.futures import ThreadPoolExecutor
import time
from topic_refresh import refresh


class RefreshQueue:
    def __init__(self,builder=refresh):
        self.executor = ThreadPoolExecutor(max_workers=1,thread_name_prefix='topic-prototype')
        self.builder = builder; self.future = None; self.dirty = False
        self.started = self.completed = self.rejected = 0; self.capture_ms = []; self.job_ms = []

    def build(self,capture):
        begin = time.perf_counter(); result = self.builder(capture)
        return result,(time.perf_counter()-begin)*1000

    def request(self,model):
        if self.future is not None: self.dirty = True; return False
        if not model.freshness()['pending_refresh']: return False
        begin = time.perf_counter(); capture = model.capture(); self.capture_ms.append((time.perf_counter()-begin)*1000)
        self.future = self.executor.submit(self.build,capture); self.started += 1; return True

    def poll(self,model,wait=False):
        if self.future is None or (not wait and not self.future.done()): return False
        future = self.future; self.future = None; snapshot,milliseconds = future.result()
        self.completed += 1; self.job_ms.append(milliseconds)
        if not model.publish(snapshot): self.rejected += 1
        if self.dirty: self.dirty = False; self.request(model)
        return True

    def drain(self,model):
        self.request(model)
        while self.future is not None: self.poll(model,wait=True)
        if model.freshness()['pending_refresh']: raise ValueError('final source snapshot was not published')

    def close(self): self.executor.shutdown(wait=True)
