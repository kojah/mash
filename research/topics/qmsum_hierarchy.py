"""Frozen cut-free hierarchy diagnostics on complete audited QMSum captures."""
import argparse
from collections import Counter
import hashlib
import json
import math
from pathlib import Path

from captured_hierarchy import Baseline,captured,evaluate,temporal
from qmsum import load,membership


CAPTURE = 'c9587e541ff38478a8c074cf2f28992d187f03ed18c15e024fbebe7d46ee0b3d'


def compare_captures(left,right):
    # No timing envelope exists here: compare every serialized field and type.
    if json.dumps(left,sort_keys=True,allow_nan=False) != json.dumps(right,sort_keys=True,allow_nan=False):
        raise ValueError('complete hierarchy repeat differs')


def components(meeting, selected):
    members,_ = membership(meeting);gold = [];groups = []
    merged = {}
    for topic in meeting.topics:
        spans = []
        for s in sorted(topic.spans,key=lambda s:(s.start,s.end)):
            if spans and s.start <= spans[-1][1]+1:spans[-1][1] = max(spans[-1][1],s.end)
            else:spans.append([s.start,s.end])
        merged[topic.ordinal] = spans
    for i in selected:
        g = members[i][0] if len(members[i]) == 1 else None;gold.append(g)
        matching = [] if g is None else [j for j,(lo,hi) in enumerate(merged[g]) if lo <= i <= hi]
        if g is not None and len(matching) != 1:raise ValueError('unique native span component required')
        groups.append(matching[0] if matching else None)
    return gold,groups


def summary(records):
    result = {}
    for order in ['forward','reverse']:
        result[order] = {}
        for method in ['grinch_style',*Baseline]:
            subset = [r for r in records if r['order'] == order and r['method'] == method]
            if len(subset) != 162:raise ValueError('complete method/order required')
            result[order][method] = {}
            for scope in ['all','far','crossspan']:
                metrics = [r['metrics'][scope] for r in subset];pairs = sum(r['pairs'] for r in metrics)
                weight = math.fsum(r['purity_sum'] for r in metrics);root = math.fsum(r['root_only_purity_sum'] for r in metrics)
                topics = [t for r in metrics for t in r['topics'] if t['pairs']]
                result[order][method][scope] = {'pairs':pairs,'purity_sum':weight,'purity':weight/pairs,
                    'root_only_purity_sum':root,'root_only_purity':root/pairs,'above_root':(weight-root)/pairs,
                    'supported_topics':len(topics),'topic_macro_purity':math.fsum(t['purity'] for t in topics)/len(topics),
                    'mean_lca_known_rows':sum(r['lca_known_rows_sum'] for r in metrics)/pairs,
                    'root_lca_pair_fraction':sum(r['root_lca_pairs'] for r in metrics)/pairs}
    return result


def experiment(source,capture):
    original = capture.read_bytes()
    if hashlib.sha256(original).hexdigest() != CAPTURE:raise ValueError('audited immutable complete capture required')
    data = json.loads(original);capture_pin,source_pin,meetings = load(source);records = [];support = Counter()
    if (data['source_sha256'] != source_pin or data['capture_sha256'] != capture_pin
            or data['method'] != 'grinch_style' or len(data['records']) != 324):raise ValueError('complete source/capture drift')
    seen = set()
    for row in data['records']:
        identity = row['meeting'],row['order']
        if identity in seen or row['order'] not in ['forward','reverse']:raise ValueError('unique complete arrival evidence')
        seen.add(identity);meeting = meetings[row['meeting']];selected = row['source_ordinals']
        if sorted(selected+row['excluded_source_ordinals']) != list(range(len(meeting.turns))):raise ValueError('source accounting drift')
        gold,spans = components(meeting,selected);tree = captured(row)
        if row['order'] == 'forward':
            support['sources'] += len(meeting.turns);support['ready'] += len(selected)
            support['known'] += sum(g is not None for g in gold);support['native_topics'] += len(meeting.topics)
        for method in ['grinch_style',*Baseline]:
            topology = tree if method == 'grinch_style' else temporal(len(selected),method)
            metrics = evaluate(topology,gold,spans,selected,len(meeting.topics))
            records.append({'meeting':meeting.ordinal,'order':row['order'],'method':method,'metrics':metrics})
        if row['meeting']%20 == 0:print(json.dumps({'meeting':row['meeting'],'order':row['order']}),flush=True)
    if support != {'sources':88336,'ready':84434,'known':77892,'native_topics':725}:raise ValueError('matched support drift')
    return {'schema_version':1,'scope':'complete_training_cut_free_conditional_hierarchy_diagnostic_not_selection',
        'source_sha256':source_pin,'source_capture_sha256':capture_pin,'discovery_sha256':CAPTURE,'support':dict(support),
        'protocol':{'geometry':'unchanged_final_audited_grinch_style_topology_both_orders',
            'metrics':'same_topic_distinct_pairs_class_fraction_at_lca_conditional_on_singleton_known',
            'scopes':['all','original_gap_greater_than4','original_gap_greater_than4_and_distinct_native_topic_components'],
            'baselines':['per_meeting_root_only',*Baseline],'temporal_order':'original_source_order_independent_of_arrival',
            'balanced':'recursive_floor_midpoint_split','combs':'successive_forward_or_reverse_join',
            'unknown_multi':'retained_in_topology_excluded_from_purity_denominator',
            'new_fits':0,'new_inference':0,'llm_calls':0,'held_out_sources':False,'semantic_ancestor_gold':False,
            'frontier_selection':False,'runtime_claim':False},
        'summary':summary(records),'records':records,'code_pins':{name:hashlib.sha256(Path(__file__).with_name(name).read_bytes()).hexdigest()
            for name in ['captured_hierarchy.py','qmsum_hierarchy.py','qmsum.py','qmsum_capture.py']}}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ['source','capture','output']:p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--compare',type=Path);a = p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    result = experiment(a.source,a.capture)
    if a.compare:compare_captures(json.loads(a.compare.read_text()),result)
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps(result['summary']),flush=True)


if __name__ == '__main__':main()
