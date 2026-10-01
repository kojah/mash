import sys
import unittest
from unittest.mock import patch

from memory_audit import kib_fields, monitor, sample


class MemoryAuditTests(unittest.TestCase):
    def test_address_space_teardown_before_zombie_is_a_vanished_sample(self):
        with patch('memory_audit.Path.read_text', return_value='Name: python\nState: R (running)\n'):
            with self.assertRaises(ProcessLookupError):
                sample(123)
        with patch('memory_audit.Path.read_text', side_effect=['VmRSS: 10 kB\nVmHWM: 20 kB\n', '']):
            with self.assertRaises(ProcessLookupError):
                sample(123)

    def test_counters_reject_duplicate_missing_and_wrong_units(self):
        self.assertEqual(kib_fields('Other: x\nRss: 1024 kB\nPss: 256 kB\n', {'Rss', 'Pss'}), {'Rss': 1024, 'Pss': 256})
        for text in ('Rss: 1 MB', 'Rss: -1 kB', 'Rss: 1 kB\nRss: 2 kB', 'Other: 1 kB'):
            with self.assertRaises(ValueError):
                kib_fields(text, {'Rss'})

    @unittest.skipUnless(sys.platform == 'linux', 'requires procfs')
    def test_observer_measures_touched_allocation_and_preserves_failure(self):
        code = 'import time; allocation=bytearray(32*1024*1024); time.sleep(.12); raise SystemExit(7)'
        report = monitor([sys.executable, '-c', code])
        self.assertEqual(report['exit_code'], 7)
        self.assertGreaterEqual(report['maximum_kib']['Rss'], 32 * 1024)
        self.assertGreaterEqual(report['sample_count'], 2)
        self.assertTrue(all(v >= 0 for s in report['samples'] for k, v in s.items()))
