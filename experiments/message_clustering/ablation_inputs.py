"""Prompt-conditioned answer excerpts with separately checked source ownership."""

from tokenizers import Tokenizer

from analyze_input_followup import validate_input_sources
from build_structure_followup import check_coverage
from corpus import Record, Role, Thread
from followup_inputs import AnswerScope, Input, Text, concat, lean_prompt, windows
from local_encoder import Selection
from structure_chunks import Kind, Prepared, bounded, tokens
from turns import Turn


class Conditioned(Record):
    body: Input
    prompt: Text
    content: Text
    selection: Selection | None
    kind: Kind
    forced_token_split: bool


def prefix(prompt: Text) -> Text:
    return (
        concat(
            (
                Text(text="Current request:\n"),
                prompt,
                Text(text="\n\nAnswer excerpt:\n"),
            ),
            separator="",
        )
        if prompt.text
        else Text(text="")
    )


def prepare(
    original: tuple[Prepared, ...],
    thread: Thread,
    turns: tuple[Turn, ...],
    tokenizer: Tokenizer,
    answer_scope: AnswerScope = AnswerScope.LAST,
) -> tuple[Conditioned, ...]:
    prompts = {}
    for turn in turns:
        if not turn.prompts:
            continue
        text = concat(tuple(lean_prompt(thread, i) for i in turn.prompts))
        first = windows(tokenizer, text, 80, overlap=0, first_only=True)
        prompts[turn.id] = first[0] if first else Text(text="")
    return condition(
        original,
        tuple(
            prompts[u.input.turn_id]
            if u.input.role == Role.ASSISTANT
            else Text(text="")
            for u in original
        ),
        thread,
        turns,
        tokenizer,
        answer_scope,
    )


def condition(
    original: tuple[Prepared, ...],
    prompts: tuple[Text, ...],
    thread: Thread,
    turns: tuple[Turn, ...],
    tokenizer: Tokenizer,
    answer_scope: AnswerScope = AnswerScope.LAST,
) -> tuple[Conditioned, ...]:
    """Condition each structural input on an explicitly selected own-turn span."""
    owners = tuple(t.id for t in turns if t.prompts)
    result = []
    for unit, prompt in zip(original, prompts, strict=True):
        item = unit.input
        before = prefix(prompt)
        budget = 500 - tokens(tokenizer, before) if before.text else 510
        pieces = (
            bounded(item.content, unit.kind, tokenizer, budget) if before.text else ()
        )
        if not before.text:
            result.append(
                Conditioned(
                    body=item,
                    prompt=prompt,
                    content=item.content,
                    selection=None,
                    kind=unit.kind,
                    forced_token_split=unit.forced_token_split,
                )
            )
            continue
        for piece in pieces:
            content = concat((before, piece.content), separator="")
            if len(tokenizer.encode(content.text).ids) > 512:
                raise ValueError("conditioned answer exceeds frozen token budget")
            body = item.model_copy(
                update={
                    "content": piece.content,
                    "tokens": len(tokenizer.encode(piece.content.text).ids),
                }
            )
            result.append(
                Conditioned(
                    body=body,
                    prompt=prompt,
                    content=content,
                    selection=Selection(start=len(before.text), end=len(content.text)),
                    kind=piece.kind,
                    forced_token_split=unit.forced_token_split
                    or piece.forced_token_split,
                )
            )
    frozen = tuple(result)
    validate(frozen, owners, thread, turns)
    check_coverage(
        tuple(u.input for u in original),
        tuple(u.body for u in frozen),
        thread,
        turns,
        answer_scope,
    )
    return frozen


def validate(
    units: tuple[Conditioned, ...],
    owners: tuple[str, ...],
    thread: Thread,
    turns: tuple[Turn, ...],
) -> None:
    validate_input_sources(tuple(u.body for u in units), owners, thread, turns)
    for unit in units:
        if unit.prompt.text:
            if unit.body.role != Role.ASSISTANT:
                raise ValueError("current-prompt conditioning is answer-only")
            validate_input_sources(
                (
                    Input(
                        turn_id=unit.body.turn_id,
                        role=Role.USER,
                        content=unit.prompt,
                        tokens=0,
                    ),
                ),
                owners,
                thread,
                turns,
            )
        before = prefix(unit.prompt)
        expected = (
            concat((before, unit.body.content), separator="")
            if before.text
            else unit.body.content
        )
        selection = (
            Selection(start=len(before.text), end=len(expected.text))
            if before.text
            else None
        )
        if unit.content != expected or unit.selection != selection:
            raise ValueError("conditioned content or pooling selection changed")
