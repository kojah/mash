"""Bounded ledger tests protect the preregistered four-policy comparison."""

import tempfile
import unittest
from pathlib import Path

import optuna

from build_token_weighting import Contract
from run_token_weighting import run_ledger
from token_weighting import Policy


class LedgerTests(unittest.TestCase):
    def test_four_trials_resume_and_reject_changed_contract(self):
        optuna.logging.set_verbosity(optuna.logging.WARNING)
        contract = Contract(
            protocol_sha256="fixed", code_sha256={}, pooled_input_sha256={}
        )
        called = []

        def evaluate(policy: Policy) -> float:
            called.append(policy)
            return 0.5

        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "ledger.sqlite"
            study = run_ledger(path, contract, evaluate)
            self.assertEqual(called, list(Policy))
            self.assertEqual(len(study.trials), 4)
            run_ledger(path, contract, evaluate)
            self.assertEqual(called, list(Policy))
            with self.assertRaisesRegex(ValueError, "contract changed"):
                run_ledger(
                    path,
                    contract.model_copy(update={"protocol_sha256": "changed"}),
                    evaluate,
                )

    def test_live_trials_require_explicit_recovery(self):
        optuna.logging.set_verbosity(optuna.logging.WARNING)
        contract = Contract(
            protocol_sha256="fixed", code_sha256={}, pooled_input_sha256={}
        )
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "ledger.sqlite"
            study = run_ledger(path, contract, lambda _: 0.5)
            study.ask()
            with self.assertRaisesRegex(ValueError, "recovery"):
                run_ledger(path, contract, lambda _: 0.5)


if __name__ == "__main__":
    unittest.main()
