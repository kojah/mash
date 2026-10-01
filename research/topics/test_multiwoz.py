import copy
import unittest

from multiwoz import Speaker, Service, parse, select


def fixture():
    schema = [{'service_name': service, 'description': 'metadata never model text',
               'slots': [{'name': slot, 'description': 'hidden slot label', 'possible_values': [], 'is_categorical': False}],
               'intents': [{'name': intent, 'description': 'hidden intent label', 'is_transactional': False,
                            'required_slots': [], 'optional_slots': {}}]}
              for service, slot, intent in (('hotel', 'hotel-name', 'find_hotel'), ('taxi', 'taxi-destination', 'book_taxi'))]

    def frame(service, intent, values=None, slots=None):
        return {'service': service, 'actions': [], 'slots': slots or [], 'state': {
            'active_intent': intent, 'requested_slots': [], 'slot_values': values or {}}}

    rows = [{'dialogue_id': 'd', 'services': ['hotel', 'taxi'], 'turns': [
        {'turn_id': '0', 'speaker': 'USER', 'utterance': 'é🙂', 'frames': [
            frame('hotel', 'find_hotel', {'hotel-name': ['é🙂', 'alias']},
                  [{'slot': 'hotel-name', 'start': 0, 'exclusive_end': 2, 'value': 'é🙂'}]), frame('taxi', 'NONE')]},
        {'turn_id': '1', 'speaker': 'SYSTEM', 'utterance': 'Certainly.', 'frames': []},
        {'turn_id': '2', 'speaker': 'USER', 'utterance': 'I need a taxi.', 'frames': [
            frame('hotel', 'NONE', {'hotel-name': ['é🙂', 'alias']}),
            frame('taxi', 'find_taxi', {'taxi-destination': ['é🙂']},
                  [{'slot': 'taxi-destination', 'copy_from': 'hotel-name', 'value': ['é🙂']}])]},
        {'turn_id': '3', 'speaker': 'SYSTEM', 'utterance': 'Done.', 'frames': []}]}]
    return schema, rows


class MultiwozTests(unittest.TestCase):
    def test_omitted_values_only_for_noncategorical_slots(self):
        schema, rows = fixture()
        del schema[0]['slots'][0]['possible_values']
        parse(schema, rows)
        schema[0]['slots'][0]['is_categorical'] = True
        with self.assertRaises(ValueError): parse(schema, rows)

    def test_exact_text_offsets_and_unknown_intents_are_preserved(self):
        corpus = parse(*fixture())
        self.assertEqual(corpus.dialogues[0].turns[0].text, 'é🙂')
        self.assertEqual((corpus.spans[0].start, corpus.spans[0].end, corpus.spans[0].byte_start, corpus.spans[0].byte_end), (0, 2, 0, 6))
        self.assertEqual(corpus.labels[0].active, (Service.HOTEL,))
        self.assertEqual(corpus.labels[0].declared_intents, ((Service.HOTEL, 'find_hotel'),))
        self.assertEqual(corpus.labels[1].active, ())
        self.assertEqual(corpus.labels[1].unknown_intents, ((Service.TAXI, 'find_taxi'),))
        self.assertFalse(corpus.copies[0].source_values_equal)
        self.assertEqual(corpus.dialogues[0].turns[1].speaker, Speaker.SYSTEM)

    def test_metadata_changes_do_not_change_model_text_or_group(self):
        schema, rows = fixture()
        original = parse(schema, rows)
        changed = copy.deepcopy(rows)
        changed[0]['turns'][0]['frames'][0]['state']['active_intent'] = 'NONE'
        schema[0]['description'] = 'a different domain label must not leak'
        other = parse(schema, changed)
        self.assertEqual(original.dialogues, other.dialogues)
        self.assertNotEqual(original.labels, other.labels)

    def test_corrupt_sources_never_get_repaired_or_partially_loaded(self):
        for mutation in ('id', 'role', 'span', 'bounds', 'copy', 'frame', 'state'):
            schema, rows = fixture()
            turns = rows[0]['turns']
            if mutation == 'id': turns[0]['turn_id'] = '00'
            elif mutation == 'role': turns[1]['speaker'] = 'USER'
            elif mutation == 'span': turns[0]['frames'][0]['slots'][0]['value'] = 'wrong'
            elif mutation == 'bounds': turns[0]['frames'][0]['slots'][0]['exclusive_end'] = True
            elif mutation == 'copy': turns[2]['frames'][1]['slots'][0]['value'] = ['wrong']
            elif mutation == 'frame': turns[0]['frames'].append(copy.deepcopy(turns[0]['frames'][0]))
            else: turns[0]['frames'][0]['state']['schema-description'] = 'feature leak'
            with self.assertRaises(ValueError, msg=mutation): parse(schema, rows)

    def test_whole_text_aliases_and_selection_do_not_depend_on_label_names(self):
        schema, rows = fixture()
        duplicate = copy.deepcopy(rows[0])
        duplicate['dialogue_id'] = 'alias'
        corpus = parse(schema, [*rows, duplicate])
        selected, duplicates = select(corpus, 'fixed', maximum=1)
        self.assertEqual((selected[0].id, duplicates), ('alias', 1))
        duplicate['turns'][0]['frames'][0]['state']['active_intent'] = 'NONE'
        with self.assertRaises(ValueError): select(parse(schema, [*rows, duplicate]), 'fixed', maximum=1)

    def test_alias_conflicts_within_same_service_are_not_hidden(self):
        schema, rows = fixture()
        extra = copy.deepcopy(schema[0]['intents'][0])
        extra['name'] = 'book_hotel'
        schema[0]['intents'].append(extra)
        duplicate = copy.deepcopy(rows[0])
        duplicate['dialogue_id'] = 'alias'
        duplicate['turns'][0]['frames'][0]['state']['active_intent'] = 'book_hotel'
        corpus = parse(schema, [*rows, duplicate])
        self.assertEqual(corpus.labels[0].active, corpus.labels[2].active)
        with self.assertRaises(ValueError): select(corpus, 'fixed', maximum=1)
