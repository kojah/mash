import unittest

from phrase_labels import source_terms, supported


class PhraseLabelTests(unittest.TestCase):
    def test_stopword_and_punctuation_do_not_create_fake_bigrams(self):
        terms = source_terms("remote control. memory and model\nphase")
        self.assertIn("remote control", terms)
        self.assertIn("model phase", terms)
        self.assertNotIn("control memory", terms)
        self.assertNotIn("memory model", terms)

    def test_technical_identifier_survives_as_one_token(self):
        self.assertIn("c-tf-idf", source_terms("compare c-TF-IDF with model_phase"))
        self.assertIn("model_phase", source_terms("compare c-TF-IDF with model_phase"))

    def test_source_support_never_joins_records_or_skips_words(self):
        self.assertTrue(supported("remote control", ["REMOTE\ncontrol works"]))
        self.assertFalse(supported("remote control", ["remote", "control"]))
        self.assertFalse(supported("memory model", ["memory and model"]))
        self.assertFalse(supported("model", ["modeling"]))
