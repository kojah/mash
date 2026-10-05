import hashlib
import tempfile
import unittest
from pathlib import Path

from click.testing import CliRunner

from clustering import CONFIGURATIONS, Configuration, Method
from corpus import Corpus, Provider, SelectedThread, Split
from frozen_settings import FrozenSettings, Selection
from run_evaluation import main
from test_corpus import fixture_thread


class FrozenSettingsContracts(unittest.TestCase):
    def test_missing_methods_mismatched_sources_and_unregistered_thresholds_fail(
        self,
    ) -> None:
        settings = FrozenSettings(
            corpus_sha256="source",
            references_sha256="references",
            development_reports_sha256=("dev",),
            selections=tuple(
                Selection(
                    configuration=next(c for c in CONFIGURATIONS if c.method == method),
                    context=True,
                    development_f1=0.5,
                )
                for method in Method
            ),
        )
        settings.verify("source", "references")
        with self.assertRaises(ValueError):
            settings.verify("different-source", "references")
        with self.assertRaises(ValueError):
            settings.model_copy(update={"selections": settings.selections[:-1]}).verify(
                "source", "references"
            )
        changed = settings.selections[0].model_copy(
            update={"configuration": Configuration(Method.LEADER, 0.4)}
        )
        with self.assertRaises(ValueError):
            settings.model_copy(
                update={"selections": (changed,) + settings.selections[1:]}
            ).verify("source", "references")

    def test_heldout_sweep_without_frozen_settings_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            corpus = Corpus(
                gold_manifest="fixture",
                source_threads=1,
                ancestry_files_checked=1,
                selected=(
                    SelectedThread(
                        split=Split.HELD_OUT,
                        component="fixture",
                        thread=fixture_thread(
                            "held", Provider.CODEX, Path("unused"), 3
                        ),
                    ),
                ),
            )
            payload = corpus.model_dump_json().encode()
            (root / "corpus.json").write_bytes(payload)
            (root / "labels.toml").write_text(
                f'schema_version=1\ncorpus_sha256="{hashlib.sha256(payload).hexdigest()}"\n'
                'provenance="synthetic fixture"\n[[threads]]\nindex=0\n'
                'related=[[0,1]]\nunrelated=[]\nnotes="fixture"\n'
            )
            result = CliRunner().invoke(
                main,
                [
                    "--corpus",
                    str(root / "corpus.json"),
                    "--references",
                    str(root / "labels.toml"),
                    "--split",
                    "held_out",
                    "--context",
                    "--output",
                    "/home/james/scratch/mash-artifacts/test-heldout-must-not-run.json",
                ],
            )
            self.assertNotEqual(result.exit_code, 0)
            self.assertIn("requires frozen development settings", result.output)


if __name__ == "__main__":
    unittest.main()
