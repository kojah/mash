"""Source-only physical boundaries, independent of embedding/lexical geometry."""
from dataclasses import dataclass
from episode_blocks import End


@dataclass(frozen=True)
class Bounds:
    ordinal: int
    start: int
    end: int
    tokens: int
    close: End


def boundaries(ready: list[bool], tokens: list[int]) -> tuple[tuple[Bounds, ...], tuple[int, ...]]:
    if (len(ready) != len(tokens) or len(ready) > 10000 or any(type(v) is not bool for v in ready)
            or any(type(v) is not int or v < 2 for v in tokens)
            or any(flag and n > 512 for flag,n in zip(ready,tokens))):
        raise ValueError('original aligned readiness and untruncated model tokens required')
    blocks = []; owners = [-1]*len(ready); start = None; total = 0
    def close(end, reason):
        nonlocal start, total
        ordinal = len(blocks); blocks.append(Bounds(ordinal,start,end,total,reason))
        owners[start:end+1] = [ordinal]*(end-start+1); start = None; total = 0
    for i,(flag,n) in enumerate(zip(ready,tokens)):
        if not flag:
            if start is not None: close(i-1,End.GAP)
            continue
        if start is not None:
            if i-start == 16: close(i-1,End.TURNS)
            elif total+n > 512: close(i-1,End.TOKENS)
        if start is None: start = i
        total += n
    if start is not None: close(len(ready)-1,End.MEETING)
    return tuple(blocks),tuple(owners)
