"""Bounded layout associations, independent of topics, queries and judgments."""
from dataclasses import dataclass
import re

from passage_scope import intervals
from source_spans import Kind


@dataclass(frozen=True)
class Section:
    head: int
    children: tuple[int, ...]
    total_children: int


def sections(text, units, maximum_children=4):
    """Link a layout intro to the immediately following list/table run only."""
    if type(maximum_children) is not int or not 1 <= maximum_children <= 16:
        raise ValueError('bounded positive child cap required')
    if units != intervals(text)['units']:
        raise ValueError('section units do not match original source')
    result = []
    for position, unit in enumerate(units):
        source = text[unit['start']:unit['end']]
        intro = (unit['kind'] == Kind.HEADING or
                 unit['kind'] == Kind.PROSE and re.search(r':(?:\*\*|__)?$', source))
        if not intro:
            continue
        children = []
        for child_position in range(position + 1, len(units)):
            candidate = units[child_position]
            if candidate['kind'] not in (Kind.LIST, Kind.TABLE):
                break
            children.append(candidate['index'])
        if children:
            result.append(Section(unit['index'], tuple(children[:maximum_children]), len(children)))
    return tuple(result)


def select(ranking, links, budget):
    """Replace an intro hit by its original children; keep a fixed unique-unit budget."""
    if type(budget) is not int or not 1 <= budget <= 16:
        raise ValueError('bounded positive unit budget required')
    available = {(entry['source_id'], entry['index']): entry for entry in ranking}
    if len(available) != len(ranking):
        raise ValueError('duplicate ranking identity')
    # Validate even associations not visited before the selection budget fills.
    for head, children in links.items():
        if (head not in available or not children or len(children) > 16
                or len(set(children)) != len(children) or head in children
                or any(child not in available or child[0] != head[0] or child[1] <= head[1] for child in children)):
            raise ValueError('section association is not unique same-source ranking evidence')
    chosen, seen, visited = [], set(), 0
    for entry in ranking:
        visited += 1
        identity = (entry['source_id'], entry['index'])
        for child in links.get(identity, (identity,)):
            if child not in seen:
                seen.add(child)
                chosen.append({'unit': available[child], 'via': {'source_id': identity[0], 'index': identity[1]},
                               'association': 'section-child' if child != identity else 'standalone'})
                if len(chosen) == budget:
                    return chosen, visited
    return chosen, visited
