"""Evaluation-only exact name-token hits; not semantic label correctness."""
from label_noise import EXCLUDED
from stable_lexical_frame import TOKEN


def evaluate(groups,labels,owners,gold,names):
    if len(groups)!=len(labels) or len(owners)!=len(gold):raise ValueError('aligned complete evaluation required')
    assignment={b:i for i,g in enumerate(groups) for b in g}
    if len(assignment)!=sum(map(len,groups)) or set(owners)!=set(assignment):raise ValueError('complete disjoint source assignment required')
    tokens=[set(t for t in TOKEN.findall(n.lower()) if t not in EXCLUDED and any(c.isalpha() for c in t)) for n in names]
    words=[{t['term'] for t in row} for row in labels];named=empty=hits=unknown=0
    for owner,topic in zip(owners,gold):
        if topic is None:unknown+=1;continue
        if type(topic) is not int or not 0<=topic<len(names):raise ValueError('known native topic required')
        if not tokens[topic]:empty+=1;continue
        named+=1;hits+=bool(words[assignment[owner]]&tokens[topic])
    return {'named_sources':named,'empty_name_sources':empty,'unknown_or_mixed_sources':unknown,'hits':hits,'rate':hits/named if named else None}
