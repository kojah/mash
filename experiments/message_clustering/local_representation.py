"""Fixed scaffolding masks and semantic context selection within one history."""

import re

import numpy as np
from tokenizers import Tokenizer

from ablation_inputs import Conditioned, condition, prepare
from corpus import Role, Thread
from experiment_study import Context
from followup_inputs import Text, concat, lean_prompt, windows
from local_encoder import Encoder, Selection
from structure_chunks import Prepared
from turn_artifact import SelectedText, Store
from turns import Turn

CODE = re.compile(r"```[\s\S]*?(?:```|$)|~~~[\s\S]*?(?:~~~|$)|`[^`\n]*`")
FILLER = re.compile(
    r"(?im)(?:^|(?<=[.!?])\s+)"
    r"(?P<filler>(?:happy to help|glad to help|thanks for asking|hello|hi|hey)"
    r"[.!](?=\s|$)|(?:i hope this helps|hope this helps|"
    r"let me know if you (?:have (?:any |more |further )?questions|"
    r"need (?:any |more |further )?help)|"
    r"feel free to ask (?:any |more |further )?questions)[.!]?(?=\s*$))"
)
SENTENCE = re.compile(r".+?(?:[.!?](?=\s|$)|\n|$)", re.DOTALL)


def filler_spans(text: str) -> tuple[Selection, ...]:
    protected = tuple((m.start(), m.end()) for m in CODE.finditer(text))
    spans = []
    for match in FILLER.finditer(text):
        start, end = match.span("filler")
        if not any(start < right and end > left for left, right in protected):
            spans.append(Selection(start=start, end=end))
    return tuple(spans)


def candidates(prompt: Text, tokenizer: Tokenizer) -> tuple[Text, ...]:
    result = []
    for match in SENTENCE.finditer(prompt.text):
        if match.group().strip():
            part = prompt.slice(match.start(), match.end())
            result.extend(windows(tokenizer, part, 80, overlap=0))
    return tuple(result)


def selected_inputs(
    units: tuple[Prepared, ...],
    original_vectors: np.ndarray,
    thread: Thread,
    turns: tuple[Turn, ...],
    encoder: Encoder,
    store: Store,
) -> tuple[tuple[Conditioned, ...], int, int]:
    """Rank current prompt spans against their own original answer chunk only."""
    if len(original_vectors) != len(units):
        raise ValueError("original vector count differs from structural units")
    choices = {}
    new_vectors = 0
    for turn in turns:
        prompt = concat(tuple(lean_prompt(thread, i) for i in turn.prompts))
        spans = candidates(prompt, encoder.tokenizer)
        vectors, added = store.vectors(
            tuple(s.text for s in spans), encoder, lambda *_: None, singleton=True
        )
        new_vectors += added
        choices[turn.id] = (spans, vectors)
    prompts = []
    accepted = 0
    for unit, vector in zip(units, original_vectors, strict=True):
        spans, vectors = choices[unit.input.turn_id]
        prompt = (
            best_context(spans, vectors, vector)
            if unit.input.role == Role.ASSISTANT
            else Text(text="")
        )
        accepted += bool(prompt.text)
        prompts.append(prompt)
    return (
        condition(units, tuple(prompts), thread, turns, encoder.tokenizer),
        new_vectors,
        accepted,
    )


def best_context(
    spans: tuple[Text, ...], vectors: np.ndarray, answer: np.ndarray
) -> Text:
    """Abstain below the frozen cosine floor; ties retain candidate source order."""
    if not spans:
        return Text(text="")
    if vectors.shape != (len(spans), len(answer)):
        raise ValueError("candidate matrix shape differs")
    scores = vectors @ answer
    best = int(np.argmax(scores))
    return spans[best] if scores[best] >= 0.20 else Text(text="")


def inputs(
    mode: Context,
    units: tuple[Prepared, ...],
    original_vectors: np.ndarray,
    thread: Thread,
    turns: tuple[Turn, ...],
    encoder: Encoder,
    store: Store,
) -> tuple[tuple[Conditioned, ...], int, int]:
    if mode == Context.SELECTED:
        return selected_inputs(units, original_vectors, thread, turns, encoder, store)
    if mode == Context.FIRST:
        result = prepare(units, thread, turns, encoder.tokenizer)
        return result, 0, sum(bool(u.prompt.text) for u in result)
    return (
        condition(
            units, (Text(text=""),) * len(units), thread, turns, encoder.tokenizer
        ),
        0,
        0,
    )


def pooling(unit: Conditioned, boilerplate: bool) -> SelectedText:
    spans = (
        filler_spans(unit.body.content.text)
        if boilerplate and unit.body.role == Role.ASSISTANT
        else ()
    )
    shift = unit.selection.start if unit.selection is not None else 0
    return SelectedText(
        text=unit.content.text,
        selection=unit.selection,
        downweighted=tuple(
            Selection(start=s.start + shift, end=s.end + shift) for s in spans
        ),
    )
