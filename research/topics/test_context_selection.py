import unittest

from context_selection import Selector, evaluate, select
from preceding_context import Message, Role, preceding
from reference_review import Judgment, Resolution


class ContextSelectionTests(unittest.TestCase):
    def test_fixed_selectors_have_distinct_causal_windows(self):
        records = tuple(Message(str(i), i, role, i - 1, text, text) for i, (role, text) in enumerate([
            (Role.USER, "original task"), (Role.ASSISTANT, "proposal"),
            (Role.USER, "yes"), (Role.ASSISTANT, "confirmation"), (Role.USER, "again")]))
        context = preceding(records, 4, maximum_users=3)
        self.assertEqual([e["ordinal"] for e in select(context, Selector.ASSISTANT)], [3])
        self.assertEqual([e["ordinal"] for e in select(context, Selector.EXCHANGE)], [2, 3])
        self.assertEqual([e["ordinal"] for e in select(context, Selector.CHAIN)], [0, 1, 2, 3])

    def test_nearest_missing_assistant_abstains_without_borrowing_older_one(self):
        context = {"display": [
            {"ordinal": 0, "role": "assistant", "text": "old", "display_status": "included"},
            {"ordinal": 1, "role": "assistant", "text": None, "display_status": "omitted-size"}]}
        self.assertEqual(select(context, Selector.ASSISTANT), ())
        self.assertEqual(len(select(context, Selector.CHAIN)), 1)

    def test_redacted_user_still_bounds_exchange(self):
        context = {"display": [
            {"ordinal": 0, "role": "assistant", "text": "old", "display_status": "included"},
            {"ordinal": 1, "role": "user", "text": "redacted", "display_status": "redacted"},
            {"ordinal": 2, "role": "assistant", "text": "new", "display_status": "included"}]}
        self.assertEqual([e["ordinal"] for e in select(context, Selector.EXCHANGE)], [2])

    def test_scoring_does_not_change_selection_and_empty_evidence_is_not_success(self):
        context = {"anchor": {"ordinal": 2}, "stop": "root", "display": [
            {"ordinal": 1, "role": "assistant", "text": "proposal", "display_status": "included"}]}
        contexts = {"cases": [{"thread": "t", "contexts": [context]}]}
        task = evaluate(contexts, (Judgment("t", 2, Resolution.TASK, (1,)),))
        prompt = evaluate(contexts, (Judgment("t", 2, Resolution.INTERACTION, ()),))
        for method in Selector:
            self.assertEqual(task[method]["rows"][0]["selected_ordinals"], prompt[method]["rows"][0]["selected_ordinals"])
            self.assertTrue(task[method]["rows"][0]["all_evidence_retained"])
            self.assertFalse(prompt[method]["rows"][0]["all_evidence_retained"])
            self.assertEqual(prompt[method]["context_on_interaction_or_unresolved"], 1)
