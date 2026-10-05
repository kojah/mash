import time
import unittest
from pathlib import Path
from unittest.mock import patch

import numpy as np
from threadpoolctl import threadpool_limits

from alternatives import SETTINGS, Alternative, Setting, dp_means, partition
from clustering import normalized_vectors, result_from_partition
from corpus import Corpus, Length, Provider, SelectedThread, Split, digest
from expanded_corpus import STRATUM_COUNTS, select_expanded
from preparation import Prepared
from references import ThreadReference
from screen_alternatives import Status, isolated_fit
from test_corpus import fixture_thread


class AlternativeContracts(unittest.TestCase):
    def test_isolated_fit_reports_timeout_without_scoring_partial_results(self) -> None:
        prepared = Prepared(np.eye(2), np.ones(2, dtype=np.bool_), (0, 1), (0, 1), 0, 0)
        reference = ThreadReference(
            index=0, related=((0, 1),), unrelated=(), notes="fixture"
        )
        with patch(
            "screen_alternatives.partition", side_effect=lambda *_: time.sleep(0.2)
        ):
            outcome = isolated_fit(
                0,
                prepared,
                reference,
                "fixture",
                "codex",
                "short",
                SETTINGS[0],
                deadline=0.01,
            )
        self.assertEqual(outcome.status, Status.TIMEOUT)
        self.assertIsNone(outcome.score)

    def test_expanded_cohort_excludes_old_components_and_keeps_reserved_split(
        self,
    ) -> None:
        threads = []
        components = []
        old = fixture_thread("old", Provider.CLAUDE, Path("unused"), 8)
        copy = fixture_thread("old-copy", Provider.CLAUDE, Path("unused"), 8)
        threads.extend((old, copy))
        components.append((old.id, copy.id))
        for (provider, length), count in STRATUM_COUNTS.items():
            messages = {Length.SHORT: 8, Length.MEDIUM: 25, Length.LONG: 151}[length]
            for index in range(count):
                identity = f"{provider}-{length}-{index}"
                threads.append(
                    fixture_thread(identity, provider, Path("unused"), messages)
                )
                components.append((identity,))
        previous = Corpus(
            gold_manifest="fixture",
            source_threads=1,
            ancestry_files_checked=1,
            selected=(
                SelectedThread(
                    split=Split.DEVELOPMENT,
                    component=digest("old"),
                    thread=old,
                ),
            ),
        )
        selected = select_expanded(tuple(threads), tuple(components), previous)
        self.assertEqual(len(selected), 72)
        self.assertEqual(
            sum(entry.split == Split.DEVELOPMENT for entry in selected), 48
        )
        self.assertEqual(sum(entry.split == Split.HELD_OUT for entry in selected), 24)
        self.assertFalse(
            {old.id, copy.id}.intersection(entry.thread.id for entry in selected)
        )
        self.assertEqual(len({entry.component for entry in selected}), 72)
        self.assertEqual(
            selected, select_expanded(tuple(threads), tuple(components), previous)
        )
        with self.assertRaises(ValueError):
            select_expanded(tuple(threads[:-1]), tuple(components[:-1]), previous)

    def test_dp_means_recovers_returning_subject_and_repeated_input(self) -> None:
        vectors = np.array([[1.0, 0.0], [0.0, 1.0], [1.0, 0.0], [0.0, 1.0]])
        labels = dp_means(vectors, 0.1)
        self.assertEqual(labels[0], labels[2])
        self.assertEqual(labels[1], labels[3])
        self.assertNotEqual(labels[0], labels[1])
        self.assertEqual(labels, dp_means(vectors, 0.1))
        # A large penalty should keep the initialized global-mean cluster.
        self.assertEqual(dp_means(vectors, 2.0), (0, 0, 0, 0))

    def test_complete_link_cannot_merge_a_low_similarity_bridge(self) -> None:
        vectors = normalized_vectors(np.array([[1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]))
        labels = partition(vectors, Setting(Alternative.COMPLETE_LINK, 0.5))
        self.assertNotEqual(labels[0], labels[2])

    def test_shared_retention_rejects_noise_and_invalid_partition(self) -> None:
        vectors = np.array([[1.0, 0.0], [1.0, 0.0], [0.0, 1.0]])
        mask = np.ones(3, dtype=np.bool_)
        result = result_from_partition(vectors, (0, 0, -1), 0.35, mask)
        self.assertEqual(len(result.groups), 1)
        self.assertEqual(result.memberships[2], ())
        with self.assertRaises(ValueError):
            result_from_partition(vectors, (0,), 0.35, mask)

    def test_library_candidates_repeat_deterministically(self) -> None:
        rng = np.random.default_rng(4)
        vectors = normalized_vectors(
            np.vstack(
                (rng.normal([1, 0], 0.03, (12, 2)), rng.normal([0, 1], 0.03, (12, 2)))
            )
        )
        with threadpool_limits(limits=1):
            for method in (
                Alternative.OPTICS,
                Alternative.SPECTRAL,
                Alternative.AFFINITY,
            ):
                setting = next(
                    setting for setting in SETTINGS if setting.method == method
                )
                with self.subTest(method=method):
                    self.assertEqual(
                        partition(vectors, setting), partition(vectors, setting)
                    )


if __name__ == "__main__":
    unittest.main()
