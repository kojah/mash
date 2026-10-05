"""Guard independent confirmation selection and protect previously reserved data."""

import unittest

from corpus import Provider, Role
from test_turns import thread
from turn_study_corpus import Phase, choose


class CohortSelectionTests(unittest.TestCase):
    def setUp(self) -> None:
        self.histories = tuple(
            thread((Role.USER, Role.ASSISTANT)).model_copy(
                update={
                    "id": f"history-{i}",
                    "provider": Provider.CLAUDE if i < 24 else Provider.CODEX,
                }
            )
            for i in range(52)
        )
        self.components: dict[str, tuple[str, ...]] = {
            t.id: (t.id,) for t in self.histories
        }

    def test_reserved_component_never_enters_either_phase(self) -> None:
        related = (self.histories[0].id, self.histories[1].id)
        self.components.update({i: related for i in related})
        chosen = choose(self.histories, self.components, {related[0]}, set())
        self.assertFalse({t.id for t, _, _ in chosen}.intersection(related))
        self.assertEqual(len({component for _, _, component in chosen}), len(chosen))

    def test_confirmation_is_fresh_even_when_provider_pool_is_exhausted(self) -> None:
        used = {t.id for t in self.histories if t.provider == Provider.CLAUDE}
        chosen = choose(self.histories, self.components, set(), used)
        confirmation = [t for t, phase, _ in chosen if phase == Phase.CONFIRMATION]
        self.assertEqual(len(confirmation), 6)
        self.assertFalse({t.id for t in confirmation}.intersection(used))
        self.assertEqual(
            chosen,
            choose(tuple(reversed(self.histories)), self.components, set(), used),
        )

    def test_small_independent_pool_fails_closed(self) -> None:
        with self.assertRaisesRegex(ValueError, "32 independent"):
            choose(self.histories[:20], self.components, set(), set())

    def test_insufficient_fresh_confirmation_fails_closed(self) -> None:
        with self.assertRaisesRegex(ValueError, "four fresh"):
            choose(
                self.histories,
                self.components,
                set(),
                {t.id for t in self.histories},
            )

    def test_two_hundred_target_and_long_thread_priority(self) -> None:
        histories = tuple(
            self.histories[i % len(self.histories)].model_copy(
                update={
                    "id": f"expanded-{i}",
                    "provider": Provider.CLAUDE if i % 2 else Provider.CODEX,
                }
            )
            for i in range(250)
        )
        components: dict[str, tuple[str, ...]] = {t.id: (t.id,) for t in histories}
        longer = frozenset(t.id for t in histories[:100])
        chosen = choose(
            histories,
            components,
            set(),
            set(),
            target=200,
            minimum=200,
            confirmation_per_provider=20,
            long_ids=longer,
        )
        self.assertEqual(len(chosen), 200)
        self.assertEqual(sum(p == Phase.CONFIRMATION for _, p, _ in chosen), 40)
        self.assertTrue(longer.issubset({t.id for t, _, _ in chosen}))
