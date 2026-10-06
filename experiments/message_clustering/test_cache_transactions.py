"""Durable cache checkpoints must not alter batch-sensitive model outputs."""

import sqlite3
import tempfile
import unittest
from pathlib import Path

import numpy as np

from clustering import VectorMatrix
from test_turns import FakeEncoder
from turn_artifact import Store


class BatchSensitiveEncoder(FakeEncoder):
    def encode(self, texts: tuple[str, ...]) -> VectorMatrix:
        self.calls += len(texts)
        return np.array([[float(len(texts)), float(text)] for text in texts])


class CacheTransactions(unittest.TestCase):
    def test_fewer_commits_preserve_inference_batches_and_durable_vectors(self) -> None:
        texts = tuple(str(i) for i in range(261))
        for singleton in (False, True):
            with self.subTest(singleton=singleton), tempfile.TemporaryDirectory() as d:
                path = Path(d) / "vectors.sqlite"
                store = Store(path)
                encoder = BatchSensitiveEncoder()
                statements: list[str] = []
                store.db.set_trace_callback(statements.append)
                progress: list[tuple[int, int]] = []
                vectors, missing = store.vectors(
                    texts,
                    encoder,
                    lambda n, total, progress=progress: progress.append((n, total)),
                    singleton,
                )
                size = 1 if singleton else 8
                reference = np.concatenate(
                    [
                        BatchSensitiveEncoder().encode(texts[offset : offset + size])
                        for offset in range(0, len(texts), size)
                    ]
                )
                np.testing.assert_array_equal(vectors, reference)
                self.assertEqual(missing, len(texts))
                self.assertEqual(statements.count("COMMIT"), 3)
                self.assertEqual(progress, [(128, 261), (256, 261), (261, 261)])
                store.close()
                store = Store(path)
                try:
                    cached, missing = store.vectors(
                        texts[::-1], encoder, lambda *_: None, singleton
                    )
                    self.assertEqual(missing, 0)
                    self.assertEqual(encoder.calls, len(texts))
                    np.testing.assert_array_equal(cached, reference[::-1])
                finally:
                    store.close()

    def test_failed_transaction_rolls_back_and_keeps_previous_checkpoint(self) -> None:
        texts = tuple(str(i) for i in range(261))
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "vectors.sqlite"
            store = Store(path)
            # Fail partway through the second INSERT transaction, after inference.
            store.db.executescript("""
                CREATE TRIGGER fail_second_checkpoint BEFORE INSERT ON vectors
                WHEN (SELECT count(*) FROM vectors) = 140
                BEGIN SELECT RAISE(ABORT, 'injected write failure'); END;
            """)
            with self.assertRaisesRegex(
                sqlite3.IntegrityError, "injected write failure"
            ):
                store.vectors(texts, BatchSensitiveEncoder(), lambda *_: None, True)
            store.close()
            store = Store(path)
            try:
                self.assertEqual(
                    store.db.execute("SELECT count(*) FROM vectors").fetchone()[0], 128
                )
                store.db.execute("DROP TRIGGER fail_second_checkpoint")
                recovered, missing = store.vectors(
                    texts, BatchSensitiveEncoder(), lambda *_: None, True
                )
                self.assertEqual(missing, 133)
                np.testing.assert_array_equal(
                    recovered,
                    np.array([[1.0, float(text)] for text in texts]),
                )
            finally:
                store.close()
