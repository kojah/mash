"""Contiguous, word-boundary model windows with complete original-text coverage."""
from dataclasses import dataclass
from enum import StrEnum
import hashlib

from tokenizers import Tokenizer


class Field(StrEnum):
    TITLE = 'title'
    BODY = 'body'
    QUERY = 'query'


def unbounded_tokenizer(raw: str) -> Tokenizer:
    """Disable persisted padding/truncation before any whole-source measurement."""
    tokenizer = Tokenizer.from_str(raw)
    tokenizer.no_truncation()
    tokenizer.no_padding()
    return tokenizer


@dataclass(frozen=True)
class Window:
    source: str
    field: Field
    ordinal: int
    start: int
    end: int
    byte_start: int
    byte_end: int
    text_sha256: str
    ids: tuple[int, ...]
    types: tuple[int, ...]
    payload_tokens: int


def windows(tokenizer: Tokenizer, source: str, field: Field, text: str) -> tuple[Window, ...]:
    if (not isinstance(source, str) or not source or not isinstance(field, Field)
            or not isinstance(text, str) or len(text.encode()) > 65536):
        raise ValueError('bounded original source identity/field/text required')
    tokenizer.no_truncation()
    tokenizer.no_padding()
    if not text.strip():
        return ()
    result, start, byte_start = [], 0, 0
    while start < len(text):
        remaining = text[start:]
        encoding = tokenizer.encode(remaining, add_special_tokens=False)
        if len(encoding.ids) <= 254:
            end = len(text)
        else:
            # A word-piece split would retokenize a suffix as a new word. Choose
            # the last whole-word boundary, retaining intervening source space.
            offsets, word_ids = encoding.offsets, encoding.word_ids
            cuts = [offsets[i][0] for i in range(1, 255)
                    if word_ids[i] != word_ids[i - 1] and offsets[i][0] > 0]
            if not cuts:
                raise ValueError('no complete-word boundary within model window')
            end = start + max(cuts)
        original = text[start:end]
        full = tokenizer.encode(original)
        payload = tokenizer.encode(original, add_special_tokens=False)
        if not 2 <= len(full.ids) <= 256 or len(full.ids) != len(payload.ids) + 2:
            raise ValueError('complete window and exactly two model special tokens required')
        byte_end = byte_start + len(original.encode())
        result.append(Window(source, field, len(result), start, end, byte_start, byte_end,
                             hashlib.sha256(original.encode()).hexdigest(), tuple(full.ids),
                             tuple(full.type_ids), len(payload.ids)))
        start, byte_start = end, byte_end
    if (''.join(text[w.start:w.end] for w in result) != text
            or byte_start != len(text.encode())):
        raise ValueError('complete original character/byte coverage required')
    return tuple(result)
