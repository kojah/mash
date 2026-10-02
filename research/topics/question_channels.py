"""Explicit HTML block-code channels with unchanged displayed-source provenance."""
from dataclasses import dataclass
from enum import StrEnum

from phrase_labels import TOKEN
from question_html import InvalidHtml, Kind, Rendered, SourceParser, render


class Channel(StrEnum):
    PROSE = 'prose'
    CODE = 'block-code'
    SEPARATOR = 'separator'
    EXCLUDED = 'excluded'


@dataclass(frozen=True)
class Run:
    channel: Channel
    start: int
    end: int
    byte_start: int
    byte_end: int


@dataclass(frozen=True)
class Channels:
    rendered: Rendered
    piece_channels: tuple[Channel, ...]
    runs: tuple[Run, ...]


@dataclass(frozen=True)
class Token:
    channel: Channel
    term: str
    start: int
    end: int


class ChannelParser(SourceParser):
    def __init__(self, source):
        super().__init__(source)
        self.pre_depth = 0
        self.channels = []

    def append(self, kind, start, end, rendered):
        super().append(kind, start, end, rendered)
        channel = Channel.EXCLUDED
        if kind == Kind.SEPARATOR:
            channel = Channel.SEPARATOR
        elif kind in (Kind.DATA, Kind.ENTITY):
            channel = Channel.CODE if self.pre_depth else Channel.PROSE
        self.channels.append(channel)

    def handle_starttag(self, tag, attrs):
        if tag == 'pre':
            if self.pre_depth:
                raise InvalidHtml('nested pre channel unsupported')
            self.pre_depth = 1
        super().handle_starttag(tag, attrs)

    def handle_endtag(self, tag):
        if tag == 'pre' and not self.pre_depth:
            raise InvalidHtml('unmatched pre channel unsupported')
        super().handle_endtag(tag)
        if tag == 'pre':
            self.pre_depth = 0


def classify(source):
    parser = ChannelParser(source)
    parser.feed(source)
    parser.close()
    if parser.pre_depth:
        raise InvalidHtml('unclosed pre channel unsupported')
    original = render(source)
    actual = Rendered(''.join(parser.parts), tuple(parser.pieces), original.source_sha256)
    if actual != original or len(parser.channels) != len(original.pieces):
        raise InvalidHtml('channel parser changed displayed-source provenance')
    runs = []
    for piece, channel in zip(original.pieces, parser.channels, strict=True):
        if piece.rendered_start == piece.rendered_end:
            continue
        if runs and runs[-1].channel == channel and runs[-1].end == piece.rendered_start:
            previous = runs.pop()
            runs.append(Run(channel, previous.start, piece.rendered_end,
                            previous.byte_start, piece.rendered_byte_end))
        else:
            runs.append(Run(channel, piece.rendered_start, piece.rendered_end,
                            piece.rendered_byte_start, piece.rendered_byte_end))
    cursor = byte_cursor = 0
    for run in runs:
        if run.start != cursor or run.byte_start != byte_cursor:
            raise InvalidHtml('incomplete channel coverage')
        cursor, byte_cursor = run.end, run.byte_end
    if cursor != len(original.text) or byte_cursor != len(original.text.encode()):
        raise InvalidHtml('incomplete channel text/byte coverage')
    return Channels(original, tuple(parser.channels), tuple(runs))


def tokens(channels):
    cursor, result = 0, []
    for match in TOKEN.finditer(channels.rendered.text):
        while cursor < len(channels.runs) and channels.runs[cursor].end <= match.start():
            cursor += 1
        if cursor == len(channels.runs):
            raise InvalidHtml('token outside channel coverage')
        run = channels.runs[cursor]
        if match.end() > run.end or run.channel not in (Channel.PROSE, Channel.CODE):
            raise InvalidHtml('token crosses source channels')
        result.append(Token(run.channel, match.group().lower(), match.start(), match.end()))
    return tuple(result)
