"""Union namespaces avoid false bridges; intersections need distinct turns."""

import unittest

from turn_ensemble import Mode, memberships, predictions


class EnsembleContracts(unittest.TestCase):
    def test_equal_group_numbers_do_not_create_a_transitive_bridge(self):
        a = {"x": frozenset((0,)), "y": frozenset((0,)), "z": frozenset()}
        b = {"x": frozenset(), "y": frozenset((0,)), "z": frozenset((0,))}
        combined = memberships(a, b, Mode.UNION)
        self.assertTrue(combined["x"] & combined["y"])
        self.assertTrue(combined["y"] & combined["z"])
        self.assertFalse(combined["x"] & combined["z"])

    def test_membership_connections_equal_the_frozen_pair_operators(self):
        a = {"x": frozenset((0, 1)), "y": frozenset((0,)), "z": frozenset((1,))}
        b = {"x": frozenset((8,)), "y": frozenset((8,)), "z": frozenset()}
        pairs = (("x", "y"), ("x", "z"), ("y", "z"))
        left = {f"{x}-{y}": bool(a[x] & a[y]) for x, y in pairs}
        right = {f"{x}-{y}": bool(b[x] & b[y]) for x, y in pairs}
        for mode in Mode:
            combined = memberships(a, b, mode)
            actual = {f"{x}-{y}": bool(combined[x] & combined[y]) for x, y in pairs}
            self.assertEqual(actual, predictions(left, right, mode))
        self.assertEqual(
            memberships(a, b, Mode.INTERSECTION)["x"], frozenset(((0, 8),))
        )

    def test_mismatched_parent_scopes_fail_closed(self):
        with self.assertRaises(ValueError):
            predictions({"one": True}, {"other": False}, Mode.UNION)
        with self.assertRaises(ValueError):
            memberships({"x": frozenset()}, {"y": frozenset()}, Mode.A)


if __name__ == "__main__":
    unittest.main()
