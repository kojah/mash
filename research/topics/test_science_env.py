from pathlib import Path
from tempfile import TemporaryDirectory
from unittest import TestCase
from science_env import read_pins,verify_manifest,validate_destination,ROOT


class ScienceEnvironmentTests(TestCase):
    def test_pins_reject_ranges_urls_and_normalized_duplicates(self):
        with TemporaryDirectory() as directory:
            path=Path(directory)/'pins.in'
            for contents in ['numpy>=2','numpy @ https://example.test/wheel','some_lib==1.0\nsome-lib==1.0']:
                path.write_text(contents)
                with self.assertRaises(ValueError):read_pins(path)
            path.write_text('# frozen\nSome_Lib==1.0\n')
            self.assertEqual(read_pins(path),{'some-lib':'1.0'})

    def test_manifest_rejects_python_package_version_and_extra_drift(self):
        base={'python':'3.11.15','implementation':'CPython','platform':'fixture','packages':[{'name':'numpy','version':'2.4.6'}]}
        self.assertEqual(len(verify_manifest(base,{'numpy':'2.4.6'})['packages']),1)
        for changes in [{'python':'3.12.0'},{'implementation':'PyPy'},
                        {'packages':[{'name':'numpy','version':'2.5.3'}]},
                        {'packages':base['packages']+[{'name':'extra','version':'1'}]},
                        {'packages':base['packages']*2}]:
            with self.assertRaises(ValueError):verify_manifest({**base,**changes},{'numpy':'2.4.6'})

    def test_initializer_rejects_repository_and_existing_destinations(self):
        with self.assertRaises(ValueError):validate_destination(ROOT/'forbidden')
        with TemporaryDirectory() as directory:
            path=Path(directory);marker=path/'retain';marker.write_text('existing state')
            with self.assertRaises(ValueError):validate_destination(path)
            self.assertEqual(marker.read_text(),'existing state')
