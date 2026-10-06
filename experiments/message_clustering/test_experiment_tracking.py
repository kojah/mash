"""Exercise real local MLflow persistence and failed-run status without inference."""

import tempfile
import unittest
from pathlib import Path

import orjson
from mlflow.tracking import MlflowClient

from experiment_tracking import Summary, log_summary, tracked_run


class TrackingContracts(unittest.TestCase):
    def test_local_run_preserves_metrics_artifact_and_failure_status(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            summary = Summary(
                treatment="first80",
                metrics={"overall.f1": 0.8},
                provenance={"contract_sha256": "fixed"},
            )
            with tracked_run(root, "test", "success", {"conditioning": "first80"}) as (
                client,
                run_id,
            ):
                log_summary(client, run_id, summary)
            restored = MlflowClient(
                tracking_uri=f"sqlite:///{root / 'tracking.sqlite'}"
            )
            run = restored.get_run(run_id)
            self.assertEqual(run.info.status, "FINISHED")
            self.assertEqual(run.data.params["conditioning"], "first80")
            self.assertEqual(run.data.metrics["overall.f1"], 0.8)
            artifact = Path(restored.download_artifacts(run_id, "summary.json"))
            self.assertEqual(
                Summary.model_validate_json(artifact.read_bytes()), summary
            )
            self.assertNotIn("rows", orjson.loads(artifact.read_bytes()))
            with self.assertRaisesRegex(ValueError, "injected failure"):
                with tracked_run(root, "test", "failure", {}) as (_, failed_id):
                    raise ValueError("injected failure")
            self.assertEqual(restored.get_run(failed_id).info.status, "FAILED")
