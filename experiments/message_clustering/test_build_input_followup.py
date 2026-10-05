"""Role-balanced turn vectors and fail-closed incomplete input coverage."""

import unittest

import numpy as np

from build_input_followup import pool
from corpus import Role
from followup_inputs import Input, Text
from test_turns import thread
from turns import extract


class FollowupPooling(unittest.TestCase):
    def test_answer_chunk_count_does_not_outweigh_prompt_view(self):
        turns = extract(thread((Role.USER, Role.ASSISTANT)))
        inputs = tuple(
            Input(turn_id=turns[0].id, role=role, content=Text(text="text"), tokens=1)
            for role in (Role.USER, Role.ASSISTANT, Role.ASSISTANT)
        )
        vectors = np.array([[1.0, 0.0], [0.0, 1.0], [0.0, 1.0]])
        np.testing.assert_allclose(pool(turns, inputs, vectors), [[2**-0.5, 2**-0.5]])

    def test_missing_view_abstains_but_missing_turn_fails(self):
        turns = extract(thread((Role.USER, Role.ASSISTANT, Role.USER)))
        inputs = (
            Input(
                turn_id=turns[0].id, role=Role.USER, content=Text(text="text"), tokens=1
            ),
        )
        np.testing.assert_array_equal(
            pool(turns[:1], inputs, np.array([[1.0, 0.0]])), [[1.0, 0.0]]
        )
        with self.assertRaises(ValueError):
            pool(turns, inputs, np.array([[1.0, 0.0]]))
        with self.assertRaises(ValueError):
            pool(turns[:1], inputs, np.empty((0, 2)))


if __name__ == "__main__":
    unittest.main()
