"""Count points within their owning histories rather than across identifiers."""

import unittest

from analyze_structure_followup import distribution
from build_structure_followup import StructuredArtifact
from corpus import Role
from followup_inputs import Input, Text
from structure_chunks import Kind, Mode, Prepared
from turn_study_corpus import Phase


class StructureDistribution(unittest.TestCase):
    def test_points_and_small_inputs_have_explicit_denominators(self):
        units = tuple(
            Prepared(
                input=Input(
                    turn_id=owner,
                    role=Role.USER,
                    content=Text(text="fixture"),
                    tokens=tokens,
                ),
                kind=kind,
                forced_token_split=forced,
            )
            for owner, tokens, kind, forced in (
                ("a", 2, Kind.HEADING, False),
                ("a", 100, Kind.PROSE, False),
                ("b", 300, Kind.CODE, True),
            )
        )
        artifact = StructuredArtifact(
            thread_index=1,
            phase=Phase.DEVELOPMENT,
            mode=Mode.PLAIN,
            turn_ids=("a", "b"),
            units=units,
            matrix_sha256="fixture",
            source_input_sha256="fixture",
            seconds=1,
            new_vectors=3,
            input_tokens=402,
        )
        other = artifact.model_copy(update={"thread_index": 2})
        result = distribution(Mode.PLAIN, (artifact, other))
        self.assertEqual((result.histories, result.turns, result.points), (2, 4, 6))
        self.assertEqual(result.points_per_turn_p50, 1.5)
        self.assertEqual(result.points_per_turn_max, 2)
        self.assertEqual(result.inputs_under_16_tokens, 2)
        self.assertEqual(result.forced_token_splits, 2)
        self.assertEqual(result.kinds, {Kind.HEADING: 2, Kind.PROSE: 2, Kind.CODE: 2})
        with self.assertRaises(ValueError):
            distribution(Mode.HEADING, (artifact,))


if __name__ == "__main__":
    unittest.main()
