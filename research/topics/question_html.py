"""Source-covered displayed-text extraction, without semantic rewriting."""
from dataclasses import dataclass
from enum import StrEnum
import html
from html.parser import HTMLParser
import re

from reference_review import digest


class Kind(StrEnum):
    DATA = 'data'
    ENTITY = 'entity'
    MARKUP = 'excluded-markup'
    COMMENT = 'excluded-comment'
    SEPARATOR = 'block-separator'


class InvalidHtml(ValueError):
    pass


@dataclass(frozen=True)
class Piece:
    kind: Kind
    start: int
    end: int
    byte_start: int
    byte_end: int
    rendered_start: int
    rendered_end: int
    rendered_byte_start: int
    rendered_byte_end: int
    source_sha256: str
    rendered_sha256: str


@dataclass(frozen=True)
class Rendered:
    text: str
    pieces: tuple[Piece, ...]
    source_sha256: str


BLOCKS = {'p', 'pre', 'blockquote', 'ul', 'ol', 'li', 'br', 'hr', 'div',
          'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'table', 'tr'}


class SourceParser(HTMLParser):
    def __init__(self, source):
        super().__init__(convert_charrefs=False)
        if not isinstance(source, str) or len(source.encode()) > 512 * 1024 or '\x00' in source:
            raise InvalidHtml('bounded HTML source required')
        self.source = source
        self.lines = [0] + [i + 1 for i, c in enumerate(source) if c == '\n']
        self.bytes = [0]
        for c in source:
            self.bytes.append(self.bytes[-1] + len(c.encode()))
        self.parts, self.pieces = [], []
        self.output_length = self.output_bytes = 0

    def source_position(self):
        line, column = self.getpos()
        return self.lines[line - 1] + column

    def append(self, kind, start, end, rendered):
        if not 0 <= start <= end <= len(self.source) or len(self.pieces) >= 4096:
            raise InvalidHtml('invalid source offset or segment cap')
        self.pieces.append(Piece(kind, start, end, self.bytes[start], self.bytes[end],
                                 self.output_length, self.output_length + len(rendered),
                                 self.output_bytes, self.output_bytes + len(rendered.encode()),
                                 digest(self.source[start:end].encode()), digest(rendered.encode())))
        self.parts.append(rendered)
        self.output_length += len(rendered)
        self.output_bytes += len(rendered.encode())
        if self.output_bytes > 65536:
            raise InvalidHtml('rendered text exceeds byte cap')

    def separator(self, start, end):
        last = next((part[-1] for part in reversed(self.parts) if part), None)
        if last is not None and last != '\n':
            self.append(Kind.SEPARATOR, start, end, '\n')

    def handle_data(self, data):
        start = self.source_position()
        if self.source[start:start + len(data)] != data:
            raise InvalidHtml('HTML data source drift')
        self.append(Kind.DATA, start, start + len(data), data)

    def entity(self, name, numeric):
        start = self.source_position()
        raw = '&' + ('#' if numeric else '') + name + ';'
        if self.source[start:start + len(raw)] != raw:
            raise InvalidHtml('unterminated or changed entity')
        self.append(Kind.ENTITY, start, start + len(raw), html.unescape(raw))

    def handle_entityref(self, name):
        self.entity(name, False)

    def handle_charref(self, name):
        self.entity(name, True)

    def opening(self, tag):
        if tag in {'script', 'style'}:
            raise InvalidHtml('active/style layout unsupported')
        start = self.source_position()
        raw = self.get_starttag_text()
        if self.source[start:start + len(raw)] != raw:
            raise InvalidHtml('HTML opening source drift')
        self.append(Kind.MARKUP, start, start + len(raw), '')
        if tag in BLOCKS:
            self.separator(start, start + len(raw))

    def handle_starttag(self, tag, attrs):
        self.opening(tag)

    def handle_startendtag(self, tag, attrs):
        self.opening(tag)

    def handle_endtag(self, tag):
        start = self.source_position()
        match = re.match(r'</[^>]*>', self.source[start:])
        if not match:
            raise InvalidHtml('HTML closing source drift')
        end = start + len(match[0])
        self.append(Kind.MARKUP, start, end, '')
        if tag in BLOCKS:
            self.separator(start, end)

    def handle_comment(self, data):
        start = self.source_position()
        raw = '<!--' + data + '-->'
        if self.source[start:start + len(raw)] != raw:
            raise InvalidHtml('HTML comment source drift')
        self.append(Kind.COMMENT, start, start + len(raw), '')

    def handle_decl(self, decl):
        raise InvalidHtml('HTML declaration unsupported')

    def handle_pi(self, data):
        raise InvalidHtml('HTML processing instruction unsupported')

    def unknown_decl(self, data):
        raise InvalidHtml('unknown HTML declaration unsupported')


def render(source):
    parser = SourceParser(source)
    parser.feed(source)
    parser.close()
    cursor = 0
    for piece in parser.pieces:
        if piece.kind != Kind.SEPARATOR:
            if piece.start != cursor:
                raise InvalidHtml('incomplete or overlapping HTML source coverage')
            cursor = piece.end
    if cursor != len(source):
        raise InvalidHtml('incomplete HTML source coverage')
    return Rendered(''.join(parser.parts), tuple(parser.pieces), digest(source.encode()))
