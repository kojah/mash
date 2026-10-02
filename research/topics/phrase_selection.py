"""Fixed source-unit candidate coverage control, not semantic MMR."""
from enum import StrEnum
import numpy as np
from subject_labels import ranked_terms


class Selection(StrEnum):
    WHOLE_THREAD='whole_thread_weight'
    SOURCE_COVERAGE='source_unit_candidate_coverage'


def select_source_coverage(values,vocabulary,rows,tokens,limit):
    scores={term:float(value) for term,value in zip(vocabulary,values) if value>0}
    # One candidate vote per distinct exact input, independent of message IDs.
    units={row.text:row for row in rows};support={}
    for unit,row in sorted(units.items()):
        local=sorted(term for term in tokens[row.id] if term in scores)
        candidates=ranked_terms(np.array([scores[term] for term in local]),local,diverse=True,limit=2) if local else []
        for candidate in candidates:support.setdefault(candidate['term'],set()).add(unit)
    remaining=set(support);selected=[];atoms=[];covered=set()
    scale=max((scores[term] for term in support),default=1.)
    while remaining and len(selected)<limit:
        eligible=[term for term in remaining if not any(set(term.split())<=prior or prior<=set(term.split()) for prior in atoms)]
        if not eligible:break
        gain={term:len(support[term]-covered) for term in eligible};maximum=max(gain.values())
        utility={term:.5*scores[term]/scale+(.5*gain[term]/maximum if maximum else 0.) for term in eligible}
        term=min(eligible,key=lambda term:(-utility[term],-scores[term],term))
        selected.append({'term':term,'score':scores[term]});atoms.append(set(term.split()))
        covered.update(support[term]);remaining.remove(term)
    return selected
