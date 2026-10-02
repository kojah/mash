"""Source-coordinate-preserving transcript-marker masking for model controls."""
from dataclasses import asdict
from enum import StrEnum

from label_noise import MARKER,Marker


class Readiness(StrEnum):
    READY='ready'
    EMPTY='empty_or_marker_only'
    WINDOW='over_model_window'


def mask(text,source_row):
    if not isinstance(text,str) or type(source_row) is not int or source_row<0:raise ValueError('original text and ordinal required')
    output=list(text);ledger=[]
    for match in MARKER.finditer(text):
        output[match.start():match.end()]=' '*(match.end()-match.start())
        ledger.append(asdict(Marker(source_row,match.group(1),match.start(),match.end(),
                                   len(text[:match.start()].encode()),len(text[:match.end()].encode()))))
    result=''.join(output)
    if len(result)!=len(text) or len(result.encode())!=len(text.encode()):raise ValueError('source coordinate drift')
    return result,ledger


def readiness(text,tokens):
    if not isinstance(text,str) or type(tokens) is not int or tokens<2:raise ValueError('exact text and special-token-inclusive count required')
    if not text.strip():return Readiness.EMPTY
    return Readiness.WINDOW if tokens>512 else Readiness.READY
