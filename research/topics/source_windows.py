"""Complete source spans with nonoverlapping, pre-filter lexical token budgets."""
from dataclasses import dataclass

import numpy as np

from phrase_labels import TOKEN
from reference_review import digest


@dataclass(frozen=True)
class Window:
    start: int
    end: int
    byte_start: int
    byte_end: int
    token_count: int
    text_sha256: str
    words: tuple[int, ...]


def windows(text, words, budget):
    if not isinstance(text, str) or not isinstance(budget, int) or isinstance(budget, bool) or budget < 1:
        raise ValueError('source string and positive integer token budget required')
    if len(words) != len(set(words)) or any(not w or ' ' in w or w != w.lower() for w in words):
        raise ValueError('unique nonempty lowercase unigram vocabulary required')
    positions = {w: i for i, w in enumerate(words)}
    tokens = list(TOKEN.finditer(text))
    boundaries = [0] + [tokens[i].start() for i in range(budget, len(tokens), budget)] + [len(text)]
    # Even token-free sources retain their complete source span and zero evidence.
    output, token_cursor, byte_cursor = [], 0, 0
    for start, end in zip(boundaries, boundaries[1:]):
        selected = tokens[token_cursor:token_cursor + budget]
        selected = [token for token in selected if token.start() < end]
        token_cursor += len(selected)
        piece = text[start:end]
        byte_end = byte_cursor + len(piece.encode())
        indexes = tuple(sorted({positions[t.group().lower()] for t in selected if t.group().lower() in positions}))
        output.append(Window(start, end, byte_cursor, byte_end, len(selected), digest(piece.encode()), indexes))
        byte_cursor = byte_end
    if token_cursor != len(tokens) or byte_cursor != len(text.encode()):
        raise ValueError('window source/token coverage incomplete')
    return output


def incidence(records, vocabulary_size):
    if len(records) > 4096 or vocabulary_size > 1024 or vocabulary_size < 0:
        raise ValueError('window incidence cap exceeded')
    output = np.zeros((len(records), vocabulary_size), dtype=bool)
    for row, record in enumerate(records):
        indexes = record.words
        if any(i < 0 or i >= vocabulary_size for i in indexes):
            raise ValueError('window word outside vocabulary')
        output[row, list(indexes)] = True
    return output
