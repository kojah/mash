"""Guard experiment resumption and provenance against accidental retuning."""

import tempfile
import unittest
from pathlib import Path

import optuna

from experiment_study import Context, Contract, Setting, run


class StudyTest(unittest.TestCase):
    def test_resume_and_contract(self) -> None:
        optuna.logging.set_verbosity(optuna.logging.WARNING)
        settings = (
            Setting(context=Context.NONE, boilerplate=False),
            Setting(context=Context.SELECTED, boilerplate=True),
        )
        contract = Contract(
            protocol_sha256="protocol",
            cohort_sha256="cohort",
            encoder_generation="model",
            settings=settings,
        )
        called = []

        def evaluate(setting: Setting) -> float:
            called.append(setting)
            return 0.8 if setting.boilerplate else 0.7

        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "study.sqlite"
            study = run(path, "frozen", contract, evaluate)
            self.assertEqual(called, list(settings))
            self.assertEqual(len(study.trials), 2)
            run(path, "frozen", contract, evaluate)
            self.assertEqual(called, list(settings))
            altered = contract.model_copy(update={"cohort_sha256": "different"})
            with self.assertRaisesRegex(ValueError, "contract changed"):
                run(path, "frozen", altered, evaluate)
            self.assertEqual(called, list(settings))


if __name__ == "__main__":
    unittest.main()
