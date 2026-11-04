import tomllib
import unittest
from typing import Literal

from pydantic import BaseModel, ConfigDict, ValidationError

from mash_sdk.config import problems


class Example(BaseModel):
    model_config = ConfigDict(extra="forbid")
    team: str
    kind: Literal["local", "shared"]


class ProblemsTests(unittest.TestCase):
    def test_validation_errors_name_fields_without_their_values(self) -> None:
        with self.assertRaises(ValidationError) as raised:
            Example.model_validate({"kind": "secret-value", "stray": "other-value"})
        found = problems(raised.exception)
        self.assertEqual([item["field"] for item in found], ["team", "kind", "stray"])
        self.assertNotIn("secret-value", str(found))
        self.assertNotIn("other-value", str(found))

    def test_file_and_syntax_errors_are_reported_for_the_whole_file(self) -> None:
        missing = problems(FileNotFoundError(2, "No such file or directory", "x"))
        self.assertEqual(
            missing, [{"field": "(file)", "problem": "No such file or directory"}]
        )
        with self.assertRaises(tomllib.TOMLDecodeError) as raised:
            tomllib.loads("team = \nkind = 1")
        [syntax] = problems(raised.exception)
        self.assertEqual(syntax["field"], "(file)")
        self.assertIn("line 1", syntax["problem"])


if __name__ == "__main__":
    unittest.main()
