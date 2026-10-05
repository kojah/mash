"""Source-preserving user/assistant turn extraction and bounded embedding inputs."""

import hashlib
from enum import StrEnum

from pydantic import Field
from tokenizers import Tokenizer

from corpus import Message, Record, Role, Thread


class Boundary(StrEnum):
    START = "start"
    USER_REPLY = "user_reply"
    BRANCH = "branch"
    ORPHAN = "orphan"


class Recipe(StrEnum):
    PROMPT = "prompt_only"
    JOINT = "joint_truncation"
    PASSAGES = "prompt_conditioned_passages"
    POOLED = "pooled_message_control"


class Span(Record):
    source: int = Field(ge=0)
    message_id: str
    start: int = Field(ge=0)
    end: int = Field(ge=0)


class Turn(Record):
    id: str
    thread_id: str
    revision: str
    prompts: tuple[int, ...]
    answers: tuple[int, ...]
    boundary: Boundary
    closed: bool

    @property
    def sources(self) -> tuple[int, ...]:
        return self.prompts + self.answers


class AssistantActivity(Record):
    ordinal: int = Field(ge=0)
    parent: int | None


class Unit(Record):
    id: str
    turn_id: str
    text: str
    spans: tuple[Span, ...]
    represented_sources: tuple[int, ...]
    tokens: int
    discarded_tokens: int = 0


def injected(message: Message) -> bool:
    return message.projection.lstrip().startswith(
        ("# AGENTS.md", "<skill>", "[gas-city]", "<hook_prompt", "<task-notification>")
    )


def extract(
    thread: Thread, activities: tuple[AssistantActivity, ...] = ()
) -> tuple[Turn, ...]:
    """Group eligible prompts/responses; a reply after response activity splits.

    Native parent outside the active source set and explicit roots split branches.
    A skipped instruction record is not an answer or a new semantic prompt.
    Omitted assistant activity counts only on the active branch or linear history.
    IDs depend on the first source identity; edits change revisions, not IDs.
    """
    turns: list[Turn] = []
    prompts: list[int] = []
    answers: list[int] = []
    boundary = Boundary.START
    responded = False
    activity_index = 0
    if tuple(sorted(activities, key=lambda activity: activity.ordinal)) != activities:
        raise ValueError("assistant activity must be in canonical order")
    if any(
        activity.parent is not None
        and (activity.parent < -1 or activity.parent >= activity.ordinal)
        for activity in activities
    ):
        raise ValueError("assistant activity parent must be earlier or root")

    def finish(closed: bool) -> None:
        nonlocal responded
        if not prompts and not answers:
            return
        sources = prompts + answers
        identity = thread.id + "\0" + thread.messages[sources[0]].id
        payload = "\0".join(
            f"{thread.messages[i].id}:{thread.messages[i].role}:"
            f"{thread.messages[i].context_parent}:{thread.messages[i].projection}"
            for i in sources
        )
        turns.append(
            Turn(
                id=hashlib.sha256(identity.encode()).hexdigest(),
                thread_id=thread.id,
                revision=hashlib.sha256(payload.encode()).hexdigest(),
                prompts=tuple(prompts),
                answers=tuple(answers),
                boundary=boundary,
                closed=closed,
            )
        )
        prompts.clear()
        answers.clear()
        responded = False

    for index, message in enumerate(thread.messages):
        while (
            activity_index < len(activities)
            and activities[activity_index].ordinal < message.ordinal
        ):
            activity = activities[activity_index]
            activity_index += 1
            active_ordinals = {thread.messages[i].ordinal for i in prompts + answers}
            if active_ordinals and (
                activity.parent is None or activity.parent in active_ordinals
            ):
                responded = True
        if not message.projection.strip() or injected(message):
            continue
        active = prompts + answers
        native_split = bool(active) and (
            message.context_parent == -1
            or (
                message.context_parent is not None
                and message.context_parent
                not in {thread.messages[i].ordinal for i in active}
            )
        )
        if native_split or (message.role == Role.USER and (answers or responded)):
            finish(True)
            boundary = Boundary.BRANCH if native_split else Boundary.USER_REPLY
        if message.role == Role.USER:
            prompts.append(index)
        else:
            if not prompts and not answers and not native_split:
                boundary = Boundary.ORPHAN
            answers.append(index)
    finish(False)
    return tuple(turns)


def joined(thread: Thread, sources: tuple[int, ...]) -> tuple[str, tuple[Span, ...]]:
    text = ""
    spans = []
    for source in sources:
        if text:
            text += "\n\n"
        start = len(text)
        text += thread.messages[source].projection
        spans.append(
            Span(
                source=source,
                message_id=thread.messages[source].id,
                start=start,
                end=len(text),
            )
        )
    return text, tuple(spans)


def slice_text(
    text: str, spans: tuple[Span, ...], start: int, end: int
) -> tuple[str, tuple[Span, ...]]:
    """Translate joined-text slices back to original Unicode character offsets."""
    selected = tuple(
        Span(
            source=span.source,
            message_id=span.message_id,
            start=max(start, span.start) - span.start,
            end=min(end, span.end) - span.start,
        )
        for span in spans
        if max(start, span.start) < min(end, span.end)
    )
    return text[start:end], selected


def windows(
    tokenizer: Tokenizer,
    text: str,
    spans: tuple[Span, ...],
    budget: int,
    overlap: int = 32,
    first_only: bool = False,
) -> tuple[tuple[str, tuple[Span, ...]], ...]:
    offsets = tokenizer.encode(text, add_special_tokens=False).offsets
    if budget <= overlap or overlap < 0:
        raise ValueError("window budget must exceed nonnegative overlap")
    if not offsets:
        return ()
    result = []
    start = 0
    while start < len(offsets):
        stop = min(start + budget, len(offsets))
        part = slice_text(text, spans, offsets[start][0], offsets[stop - 1][1])
        # A slice can start inside a WordPiece word. Re-tokenization may add
        # tokens; enforce the actual input budget rather than trusting offsets.
        while len(tokenizer.encode(part[0], add_special_tokens=False).ids) > budget:
            stop -= 1
            if stop <= start:
                raise ValueError("one source token cannot fit the window budget")
            part = slice_text(text, spans, offsets[start][0], offsets[stop - 1][1])
        result.append(part)
        if first_only or stop == len(offsets):
            break
        start = max(start + 1, stop - overlap)
    return tuple(result)


def units(
    thread: Thread, turn: Turn, recipe: Recipe, tokenizer: Tokenizer
) -> tuple[Unit, ...]:
    """Generate inputs without implicit truncation; raw tokenizer has no padding."""
    prompts = joined(thread, turn.prompts)
    answers = joined(thread, turn.answers)
    inputs: list[tuple[str, tuple[Span, ...], tuple[int, ...], int]] = []
    if recipe == Recipe.POOLED:
        return ()
    if recipe == Recipe.JOINT:
        text, spans = joined(thread, turn.sources)
        count = len(tokenizer.encode(text, add_special_tokens=False).ids)
        first = windows(tokenizer, text, spans, 510, first_only=True)
        if first:
            kept = len(tokenizer.encode(first[0][0], add_special_tokens=False).ids)
            inputs.append((*first[0], turn.sources, max(0, count - kept)))
    elif recipe == Recipe.PROMPT:
        text, spans = prompts if turn.prompts else answers
        for part, source_spans in windows(tokenizer, text, spans, 510):
            inputs.append((part, source_spans, turn.sources, 0))
    else:
        if turn.prompts:
            for part, source_spans in windows(tokenizer, *prompts, 510):
                inputs.append((part, source_spans, turn.prompts, 0))
        context = windows(tokenizer, *prompts, 96, overlap=0, first_only=True)
        prefix, prefix_spans = context[0] if context else ("", ())
        prefix_tokens = len(tokenizer.encode(prefix, add_special_tokens=False).ids)
        # Leave four tokens for re-tokenization effects at the joined boundary.
        for part, source_spans in windows(tokenizer, *answers, 506 - prefix_tokens):
            text = prefix + "\n\n" + part if prefix else part
            represented = tuple(
                sorted({span.source for span in source_spans} | set(turn.prompts))
            )
            inputs.append((text, prefix_spans + source_spans, represented, 0))
    result = []
    for index, (text, spans, represented, dropped) in enumerate(inputs):
        tokens = len(tokenizer.encode(text, add_special_tokens=True).ids)
        if tokens > 512:
            raise ValueError("prepared turn passage exceeds model tokenizer budget")
        result.append(
            Unit(
                id=hashlib.sha256(f"{turn.id}:{recipe}:{index}".encode()).hexdigest(),
                turn_id=turn.id,
                text=text,
                spans=spans,
                represented_sources=represented,
                tokens=tokens,
                discarded_tokens=dropped,
            )
        )
    return tuple(result)
