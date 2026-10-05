"""A changed control report fails before corpus/annotation access."""

import tempfile
import unittest
from pathlib import Path

from click.testing import CliRunner

from run_structure_followup import main


class StructurePreflight(unittest.TestCase):
    def test_changed_source_report_rejects_before_parsing_any_other_input(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            invalid = root / "invalid.json"
            invalid.write_text("not JSON and not the frozen report")
            output = root / "output.json"
            arguments = [
                value
                for flag in (
                    "--cohort",
                    "--protocol",
                    "--annotations",
                    "--artifacts",
                    "--source-development",
                )
                for value in (flag, str(invalid))
            ]
            result = CliRunner().invoke(main, arguments + ["--output", str(output)])
            self.assertEqual(result.exit_code, 1)
            self.assertIn("frozen window report changed", result.output)
            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
