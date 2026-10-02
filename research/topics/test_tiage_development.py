from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from tiage_development import capture,paired_baseline


class TiageDevelopmentTests(unittest.TestCase):
    def test_development_download_is_blocked_before_network_when_calibration_not_sealed(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);first=root/'first';second=root/'second';first.mkdir();second.mkdir()
            for p in [first,second]:(p/'results.json').write_text('{}')
            destination=root/'development'
            with patch('tiage_development.urllib.request.urlopen') as network:
                with self.assertRaisesRegex(ValueError,'sealed pre-development'):capture(first,second,destination)
                network.assert_not_called()
            self.assertFalse(destination.exists())

    def test_paired_resampling_preserves_equal_baseline_and_known_no_change_loss(self):
        all_change=[{'tp':2,'fp':3,'fn':0,'tn':0}]*3
        self.assertEqual(paired_baseline(all_change)['f1_minus_all_change_percentiles'],[0.,0.,0.])
        no_change=[{'tp':0,'fp':0,'fn':2,'tn':3}]*3
        self.assertEqual(paired_baseline(no_change)['f1_minus_all_change_percentiles'],[-4/7]*3)


if __name__=='__main__':unittest.main()
