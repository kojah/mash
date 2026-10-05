"""Source-preserving Markdown structure, with explicit oversized-atom fallback."""

import re
from dataclasses import dataclass
from enum import StrEnum

from tokenizers import Tokenizer

from corpus import Record, Role, Thread
from followup_inputs import (
    Input,
    Text,
    concat,
    context_prefix,
    lean_prompt,
    source_text,
    windows,
)
from turns import Turn

HEAD = re.compile(r"^ {0,3}#{1,6}(?:\s|$)")
FENCE = re.compile(r"^ {0,3}(`{3,}|~{3,})")
SENTENCE = re.compile(r"(?<=[.!?])\s+(?=[A-Z0-9\"'(\[])")


class Kind(StrEnum):
    PROSE = "prose"
    CODE = "code"
    HEADING = "heading"


class Mode(StrEnum):
    PLAIN = "structure_plain"
    HEADING = "structure_heading"


@dataclass(frozen=True)
class Block:
    start: int
    end: int
    kind: Kind
    section: int
    heading: tuple[int, int] | None


def trim(text: Text) -> Text:
    left = len(text.text) - len(text.text.lstrip())
    return text.slice(left, max(left, len(text.text.rstrip())))


def blocks(text: str) -> tuple[Block, ...]:
    result = []
    offset = 0
    prose = None
    code = None
    fence = ""
    section = 0
    heading = None
    for line in text.splitlines(keepends=True):
        end = offset + len(line)
        marker = FENCE.match(line)
        if code is not None:
            if (
                marker
                and marker[1][0] == fence[0]
                and len(marker[1]) >= len(fence)
                and not line[marker.end() :].strip()
            ):
                result.append(Block(code, end, Kind.CODE, section, heading))
                code = None
        elif HEAD.match(line):
            if prose is not None:
                result.append(Block(prose, offset, Kind.PROSE, section, heading))
                prose = None
            section += 1
            heading = (offset, end)
            result.append(Block(offset, end, Kind.HEADING, section, heading))
        elif marker:
            if prose is not None:
                result.append(Block(prose, offset, Kind.PROSE, section, heading))
                prose = None
            code = offset
            fence = marker[1]
        elif not line.strip():
            if prose is not None:
                result.append(Block(prose, offset, Kind.PROSE, section, heading))
                prose = None
        elif prose is None:
            prose = offset
        offset = end
    if code is not None:
        result.append(Block(code, len(text), Kind.CODE, section, heading))
    elif prose is not None:
        result.append(Block(prose, len(text), Kind.PROSE, section, heading))
    return tuple(result)


def tokens(tokenizer: Tokenizer, text: Text) -> int:
    return len(tokenizer.encode(text.text, add_special_tokens=False).ids)


@dataclass(frozen=True)
class Body:
    content: Text
    kind: Kind
    forced_token_split: bool = False


def bounded(
    text: Text, kind: Kind, tokenizer: Tokenizer, budget: int
) -> tuple[Body, ...]:
    if tokens(tokenizer, text) <= budget:
        return (Body(trim(text), kind),) if text.text.strip() else ()
    boundaries = (
        tuple(match.end() for match in re.finditer(r"\n", text.text))
        if kind == Kind.CODE
        else tuple(match.end() for match in SENTENCE.finditer(text.text))
    )
    result = []
    pending = None
    start = 0
    for end in (*boundaries, len(text.text)):
        if end <= start:
            continue
        atom = text.slice(start, end)
        if tokens(tokenizer, atom) > budget:
            if pending is not None:
                result.append(Body(trim(text.slice(pending, start)), kind))
                pending = None
            result.extend(
                Body(trim(part), kind, True)
                for part in windows(tokenizer, atom, budget, overlap=32)
            )
        elif pending is None:
            pending = start
        elif tokens(tokenizer, text.slice(pending, end)) > budget:
            result.append(Body(trim(text.slice(pending, start)), kind))
            pending = start
        start = end
    if pending is not None:
        result.append(Body(trim(text.slice(pending, len(text.text))), kind))
    return tuple(body for body in result if body.content.text.strip())


def contains(body: Text, heading: Text) -> bool:
    return all(
        any(
            location.source.message_id == required.source.message_id
            and location.source.start <= required.source.start
            and location.source.end >= required.source.end
            for location in body.locations
        )
        for required in heading.locations
    )


class Prepared(Record):
    input: Input
    kind: Kind
    forced_token_split: bool


def prepare_structured(
    thread: Thread,
    turns: tuple[Turn, ...],
    current: int,
    mode: Mode,
    tokenizer: Tokenizer,
) -> tuple[Prepared, ...]:
    turn = turns[current]
    parent, context_id = context_prefix(thread, turns, current, tokenizer)
    answers = tuple(
        index for index in turn.answers if thread.messages[index].projection.strip()
    )
    selected = tuple((Role.USER, lean_prompt(thread, index)) for index in turn.prompts)
    if answers:
        selected += ((Role.ASSISTANT, source_text(thread, answers[-1])),)
    result = []
    for role, text in selected:
        parsed = blocks(text.text)
        sections = sorted({block.section for block in parsed})
        for section in sections:
            parts = tuple(block for block in parsed if block.section == section)
            heading = (
                trim(text.slice(*parts[0].heading))
                if parts[0].heading
                else Text(text="")
            )
            bounded_heading = windows(
                tokenizer, heading, 32, overlap=0, first_only=True
            )
            heading_prefix = (
                concat(
                    (Text(text="Section:\n"), bounded_heading[0], Text(text="\n")),
                    separator="",
                )
                if mode == Mode.HEADING and bounded_heading
                else Text(text="")
            )
            base = parent if role == Role.USER else Text(text="")
            reserved = concat((base, heading_prefix), separator="")
            budget = 506 - tokens(tokenizer, reserved) if reserved.text else 510
            planned = []
            pending = []
            for block in parts:
                if block.kind == Kind.HEADING:
                    if pending:
                        planned.append((pending[0].start, pending[-1].end, Kind.PROSE))
                    pending = [block]
                    continue
                combined = (
                    text.slice(pending[0].start, block.end)
                    if pending
                    else text.slice(block.start, block.end)
                )
                heading_only = len(pending) == 1 and pending[0].kind == Kind.HEADING
                if block.kind == Kind.CODE:
                    if heading_only and tokens(tokenizer, combined) <= budget:
                        planned.append((pending[0].start, block.end, Kind.CODE))
                    else:
                        if pending:
                            planned.append(
                                (pending[0].start, pending[-1].end, Kind.PROSE)
                            )
                        planned.append((block.start, block.end, Kind.CODE))
                    pending = []
                elif pending and (
                    tokens(tokenizer, combined) <= min(128, budget)
                    or heading_only
                    and tokens(tokenizer, combined) <= budget
                ):
                    pending.append(block)
                else:
                    if pending:
                        planned.append((pending[0].start, pending[-1].end, Kind.PROSE))
                    pending = [block]
            if pending:
                kind = (
                    Kind.HEADING
                    if len(pending) == 1 and pending[0].kind == Kind.HEADING
                    else Kind.PROSE
                )
                planned.append((pending[0].start, pending[-1].end, kind))
            for left, right, kind in planned:
                for body in bounded(text.slice(left, right), kind, tokenizer, budget):
                    own_heading = (
                        heading_prefix
                        if not contains(body.content, heading)
                        else Text(text="")
                    )
                    content = concat((base, own_heading, body.content), separator="")
                    count = len(tokenizer.encode(content.text).ids)
                    if count > 512:
                        raise ValueError(
                            "structured input exceeds the frozen encoder limit"
                        )
                    result.append(
                        Prepared(
                            input=Input(
                                turn_id=turn.id,
                                role=role,
                                content=content,
                                context_turn_id=context_id
                                if role == Role.USER
                                else None,
                                tokens=count,
                            ),
                            kind=body.kind,
                            forced_token_split=body.forced_token_split,
                        )
                    )
    return tuple(result)
