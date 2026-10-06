"""Source-preserving lean selections and structurally bounded prompt context."""

import re
from enum import StrEnum

import orjson
from pydantic import TypeAdapter, ValidationError
from tokenizers import Tokenizer

from corpus import Record, Role, Thread
from local_encoder import Selection
from turns import Boundary, Span, Turn

OPEN_REPLY = "<send_user_message_question_reply>"
CLOSE_REPLY = "</send_user_message_question_reply>"
JSON_STRING = re.compile(r'"(?:[^"\\]|\\.)*"')


class Variant(StrEnum):
    RAW = "raw_separate"
    LEAN = "lean_separate"
    CONTEXT_MEAN = "context_whole"
    CONTEXT_TARGET = "context_current_prompt"


class AnswerScope(StrEnum):
    ALL = "all"
    LAST = "last"


class Reply(Record):
    questionItemId: str
    answer: str
    question: str | None = None


REPLIES = TypeAdapter(tuple[Reply, ...])


class Located(Record):
    start: int
    end: int
    source: Span


class Text(Record):
    text: str
    locations: tuple[Located, ...] = ()

    def slice(self, start: int, end: int) -> "Text":
        if not 0 <= start <= end <= len(self.text):
            raise ValueError("slice is outside source-preserving text")
        located = []
        for item in self.locations:
            left, right = max(start, item.start), min(end, item.end)
            if left >= right:
                continue
            located.append(
                Located(
                    start=left - start,
                    end=right - start,
                    source=item.source.model_copy(
                        update={
                            "start": item.source.start + left - item.start,
                            "end": item.source.start + right - item.start,
                        }
                    ),
                )
            )
        return Text(text=self.text[start:end], locations=tuple(located))


def concat(parts: tuple[Text, ...], separator: str = "\n\n") -> Text:
    text = ""
    locations = []
    for part in parts:
        if not part.text:
            continue
        if text:
            text += separator
        shift = len(text)
        text += part.text
        locations.extend(
            item.model_copy(
                update={"start": item.start + shift, "end": item.end + shift}
            )
            for item in part.locations
        )
    return Text(text=text, locations=tuple(locations))


def source_text(thread: Thread, index: int) -> Text:
    message = thread.messages[index]
    return Text(
        text=message.projection,
        locations=(
            Located(
                start=0,
                end=len(message.projection),
                source=Span(
                    source=index,
                    message_id=message.id,
                    start=0,
                    end=len(message.projection),
                ),
            ),
        ),
    )


def lean_prompt(thread: Thread, index: int) -> Text:
    """Validate supported client envelopes; copy scalar literals at exact offsets.

    Escape sequences remain verbatim rather than inventing offset mappings for
    decoded characters. Unknown schemas and empty semantic selections keep raw text.
    """
    original = source_text(thread, index)
    stripped = original.text.strip()
    if not (stripped.startswith(OPEN_REPLY) and stripped.endswith(CLOSE_REPLY)):
        return original
    base = len(original.text) - len(original.text.lstrip()) + len(OPEN_REPLY)
    stop = len(original.text.rstrip()) - len(CLOSE_REPLY)
    body = original.text[base:stop]
    try:
        records = REPLIES.validate_json(body)
    except ValidationError:
        return original
    if not records:
        return original
    tokens = tuple(JSON_STRING.finditer(body))
    pieces = []
    for position, token in enumerate(tokens[:-1]):
        key = orjson.loads(token.group())
        suffix = body[token.end() :]
        if key not in ("question", "answer") or not suffix.lstrip().startswith(":"):
            continue
        value_start = token.end() + len(suffix) - len(suffix.lstrip()) + 1
        while value_start < len(body) and body[value_start].isspace():
            value_start += 1
        value = tokens[position + 1]
        if value.start() != value_start:
            continue
        pieces.append(original.slice(base + value.start() + 1, base + value.end() - 1))
    selected = concat(tuple(pieces))
    return selected if selected.text.strip() else original


class Ancestor(Record):
    turn_id: str
    sources: tuple[int, ...]


def ancestor(thread: Thread, turns: tuple[Turn, ...], current: int) -> Ancestor | None:
    turn = turns[current]
    if not turn.prompts:
        return None
    first = thread.messages[turn.prompts[0]]
    parent = first.context_parent
    if parent == -1:
        return None
    if parent is None:
        linear = all(message.context_parent is None for message in thread.messages)
        if not linear or current == 0 or turn.boundary != Boundary.USER_REPLY:
            return None
        prior = turns[current - 1]
        return Ancestor(turn_id=prior.id, sources=prior.sources)
    for prior in reversed(turns[:current]):
        if any(thread.messages[index].ordinal == parent for index in prior.sources):
            # A branch can point into the middle of a previous turn. Its later
            # assistant segments are siblings, not context for this branch.
            sources = tuple(
                index
                for index in prior.sources
                if thread.messages[index].ordinal <= parent
            )
            return Ancestor(turn_id=prior.id, sources=sources)
    return None


def windows(
    tokenizer: Tokenizer,
    text: Text,
    budget: int,
    overlap: int = 32,
    first_only: bool = False,
    preserve_edges: bool = False,
) -> tuple[Text, ...]:
    """Same budget/retokenization rule as turns.windows, with nonzero source origins."""
    if budget <= overlap or overlap < 0:
        raise ValueError("window budget must exceed overlap")
    offsets = tokenizer.encode(text.text, add_special_tokens=False).offsets
    if not offsets:
        return (text,) if preserve_edges and text.text.strip() else ()

    def piece(start: int, stop: int) -> Text:
        left = 0 if preserve_edges and start == 0 else offsets[start][0]
        right = (
            len(text.text)
            if preserve_edges and stop == len(offsets)
            else offsets[stop - 1][1]
        )
        return text.slice(left, right)

    result = []
    start = 0
    while start < len(offsets):
        stop = min(start + budget, len(offsets))
        part = piece(start, stop)
        while len(tokenizer.encode(part.text, add_special_tokens=False).ids) > budget:
            stop -= 1
            if stop <= start:
                raise ValueError("source token cannot fit budget")
            part = piece(start, stop)
        result.append(part)
        if first_only or stop == len(offsets):
            break
        start = max(start + 1, stop - overlap)
    return tuple(result)


class Input(Record):
    turn_id: str
    role: Role
    content: Text
    target: Selection | None = None
    context_turn_id: str | None = None
    tokens: int


def context_prefix(
    thread: Thread,
    turns: tuple[Turn, ...],
    current: int,
    tokenizer: Tokenizer,
) -> tuple[Text, str | None]:
    """The frozen 80-token prompt/80-token answer ancestor context."""
    previous = ancestor(thread, turns, current)
    prefix = Text(text="")
    if previous:
        prompts = concat(
            tuple(
                lean_prompt(thread, index)
                for index in previous.sources
                if thread.messages[index].role == Role.USER
            )
        )
        answers = tuple(
            index
            for index in previous.sources
            if thread.messages[index].role == Role.ASSISTANT
            and thread.messages[index].projection.strip()
        )
        prior_answer = source_text(thread, answers[-1]) if answers else Text(text="")
        parts = []
        for label, text in (
            ("Previous user:\n", prompts),
            ("Previous assistant:\n", prior_answer),
        ):
            bounded = windows(tokenizer, text, 80, overlap=0, first_only=True)
            if bounded:
                parts.extend((Text(text=label), bounded[0], Text(text="\n")))
        if parts:
            parts.append(Text(text="Current user:\n"))
            prefix = concat(tuple(parts), separator="")
    return prefix, previous.turn_id if previous and prefix.text else None


def prepare(
    thread: Thread,
    turns: tuple[Turn, ...],
    current: int,
    variant: Variant,
    tokenizer: Tokenizer,
    answer_scope: AnswerScope = AnswerScope.ALL,
) -> tuple[Input, ...]:
    if variant == Variant.RAW:
        raise ValueError("raw control must use frozen original matrices")
    turn = turns[current]
    prompt = concat(tuple(lean_prompt(thread, index) for index in turn.prompts))
    answer_indices = tuple(
        index for index in turn.answers if thread.messages[index].projection.strip()
    )
    selected_answers = (
        answer_indices[-1:] if answer_scope == AnswerScope.LAST else answer_indices
    )
    answer = concat(tuple(source_text(thread, i) for i in selected_answers))
    prefix, context_id = (
        (Text(text=""), None)
        if variant == Variant.LEAN
        else context_prefix(thread, turns, current, tokenizer)
    )
    prefix_tokens = len(tokenizer.encode(prefix.text, add_special_tokens=False).ids)
    result = []
    for role, text, budget in (
        (Role.USER, prompt, 506 - prefix_tokens if prefix.text else 510),
        (Role.ASSISTANT, answer, 510),
    ):
        for part in windows(
            tokenizer,
            text,
            budget,
            preserve_edges=answer_scope == AnswerScope.ALL and role == Role.ASSISTANT,
        ):
            included = prefix if role == Role.USER else Text(text="")
            content = concat((included, part), separator="")
            target = (
                Selection(start=len(included.text), end=len(content.text))
                if role == Role.USER and variant == Variant.CONTEXT_TARGET
                else None
            )
            tokens = len(tokenizer.encode(content.text).ids)
            if tokens > 512:
                raise ValueError("context input exceeds tokenizer budget")
            result.append(
                Input(
                    turn_id=turn.id,
                    role=role,
                    content=content,
                    target=target,
                    context_turn_id=context_id if included.text else None,
                    tokens=tokens,
                )
            )
    return tuple(result)
