"""Audit lexical boundary alternatives; maximal runs are not semantic tokens."""
from collections import Counter
import unicodedata

from sklearn.feature_extraction.text import ENGLISH_STOP_WORDS

from phrase_labels import TOKEN


PUNCTUATION = frozenset('.+:/#-')


def character(value):
    return value.isalnum() or value == '_' or unicodedata.category(value).startswith('M') or value in PUNCTUATION


def audit(text, offset=0, byte_offset=0, vocabulary=(), graph_words=()):
    if not isinstance(text, str) or len(text.encode()) > 65536:
        raise ValueError('bounded source text required')
    if type(offset) is not int or type(byte_offset) is not int or min(offset, byte_offset) < 0:
        raise ValueError('nonnegative source offsets required')
    vocabulary, graph_words = set(vocabulary), set(graph_words)
    prefix = [0]
    for c in text:
        prefix.append(prefix[-1] + len(c.encode()))
    spans, start = [], None
    for i in range(len(text) + 1):
        if i < len(text) and character(text[i]):
            if start is None:
                start = i
        elif start is not None:
            if any(c.isalnum() or c == '_' for c in text[start:i]):
                spans.append((start, i))
            start = None
    result, matched = [], []
    for start, end in spans:
        surface = text[start:end]
        matches = []
        for match in TOKEN.finditer(surface):
            term = match.group().lower()
            state = ('stopword' if term in ENGLISH_STOP_WORDS else
                     'outside-original-vocabulary' if term not in vocabulary else
                     'outside-graph-vocabulary' if term not in graph_words else 'graph-retained')
            a, b = start + match.start(), start + match.end()
            matches.append({'surface': match.group(), 'term': term, 'state': state,
                            'start': offset + a, 'end': offset + b,
                            'byte_start': byte_offset + prefix[a], 'byte_end': byte_offset + prefix[b]})
            matched.append((a, b, term))
        result.append({'surface': surface, 'start': offset + start, 'end': offset + end,
                       'byte_start': byte_offset + prefix[start], 'byte_end': byte_offset + prefix[end],
                       'matches': matches, 'exact_single_match': len(matches) == 1 and matches[0]['surface'] == surface})
    expected = [(m.start(), m.end(), m.group().lower()) for m in TOKEN.finditer(text)]
    if matched != expected:
        raise ValueError('maximal-run audit changed original TOKEN inventory')
    return result


def summarize(records):
    counts, states, variants = Counter(), Counter(), {}
    for record in records:
        for row in record['spans']:
            counts['maximal_runs'] += 1
            counts['boundary_different_runs'] += not row['exact_single_match']
            counts['multi_match_runs'] += len(row['matches']) > 1
            counts['zero_match_runs'] += not row['matches']
            counts['plus_hash_runs'] += '+' in row['surface'] or '#' in row['surface']
            for match in row['matches']:
                counts['original_matches'] += 1
                states[match['state']] += 1
                variants.setdefault(match['term'], set()).add(row['surface'].lower())
    collisions = {term: sorted(forms) for term, forms in sorted(variants.items()) if len(forms) > 1}
    return {'counts': dict(sorted(counts.items())), 'states': dict(sorted(states.items())),
            'normalized_terms_with_multiple_maximal_forms': collisions}
