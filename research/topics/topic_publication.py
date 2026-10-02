"""Research-only coalesced publication of the unchanged phrase reference."""
from dataclasses import dataclass
import os
import resource
import time

from phrase_selection import Selection
from topic_phrase_store import PhraseStore


@dataclass(frozen=True)
class SnapshotResult:
    snapshot: dict
    wall_ms: float
    cpu_ms: float
    peak_rss_kib: int
    pid: int


def build_snapshot(checkpoint):
    started = time.perf_counter()
    cpu = time.process_time()
    model = PhraseStore.restore(checkpoint)
    snapshot = model.snapshot(Selection.SOURCE_COVERAGE)
    return SnapshotResult(snapshot, 1000 * (time.perf_counter() - started),
                          1000 * (time.process_time() - cpu),
                          resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,
                          os.getpid())


class Publisher:
    """One in-flight checkpoint; pending work is only a latest revision marker.

    The caller owns the executor and polls after ledger mutations. A completed
    historical revision is a self-consistent snapshot, not the current ledger.
    No threads mutate the parent ledger. Errors do not overwrite publication.
    """
    def __init__(self, model, submit):
        self.model = model
        self.submit = submit
        self.published = None
        self.published_revision = -1
        self.failed = False
        self._future = None
        self._inflight_revision = None

    @property
    def jobs(self):
        return int(self._future is not None)

    @property
    def pending(self):
        target = (self._inflight_revision if self._future is not None
                  else self.published_revision)
        return int(self.model.revision > target)

    @property
    def drained(self):
        return (not self.failed and self._future is None
                and self.published_revision == self.model.revision)

    def poll(self):
        completed = None
        if self._future is not None and self._future.done():
            future, revision = self._future, self._inflight_revision
            self._future = self._inflight_revision = None
            try:
                completed = future.result()
                snapshot = completed.snapshot
                if (snapshot['revision'] != revision
                        or snapshot['namespace'] != self.model.namespace
                        or revision <= self.published_revision):
                    raise ValueError('snapshot revision or namespace differs')
            except Exception:
                self.failed = True
                raise
            self.published = completed
            self.published_revision = revision
        if not self.failed and self._future is None and self.pending:
            checkpoint = self.model.checkpoint()
            try:
                self._future = self.submit(checkpoint)
            except Exception:
                self.failed = True
                raise
            self._inflight_revision = checkpoint['revision']
        return completed

    def retry(self):
        if not self.failed:
            raise ValueError('retry requires a failed publication')
        self.failed = False
        return self.poll()
