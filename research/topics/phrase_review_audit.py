"""Independent original-review support arithmetic; no producer imports."""
import argparse
import hashlib
import json
import math
from pathlib import Path
from topic_capacity_audit import counts


def audit_data(packet,review,a):
    snapshot=a['snapshot'];sources={};subjects=[];by_thread={c['thread']:c for c in review['cases']}
    for case in packet['cases']:
        accepted={}
        for turn,judgment in zip(case['turns'],by_thread[case['thread']]['admission_judgments']):
            if judgment['kind'] in {'content','referential'}:
                assert not turn['display_redacted']
                assert hashlib.sha256(turn['text'].encode()).hexdigest()==turn['text_sha256']==judgment['text_sha256']
                sid=json.dumps([case['thread'],turn['id']],ensure_ascii=False,separators=(',',':'))
                accepted[turn['ordinal']]=sid;sources[sid]=turn['text']
        for i,subject in enumerate(by_thread[case['thread']]['subjects']):
            if subject['importance']!='uncertain':subjects.append((case['thread'],i,subject,{accepted[o] for o in subject['support_ordinals']}))
    assert set(snapshot['memberships'])==set(snapshot['versions'])==set(sources)
    words={sid:counts(text) for sid,text in sources.items()};topics={t['id']:t for t in snapshot['topics']};threads={t['thread']:t for t in snapshot['threads']}
    for thread in threads.values():
        for sid in thread['sources']:
            expected={tag['topic'] for tag in thread['tags'] if topics[tag['topic']]['phrase'] in words[sid]}
            assert expected=={tag['topic'] for tag in snapshot['memberships'][sid]}
    checked=0;summary={k:[] for k in ['primary','secondary']};maximum_error=0.
    for (thread,i,subject,expected),row in zip(subjects,a['subjects']):
        assert (thread,i,subject['importance'])==(row['thread'],row['subject_ordinal'],row['importance'])
        local=set(threads[thread]['sources']);recalled=sum(bool(snapshot['memberships'][sid]) for sid in expected)
        values={}
        for tag in threads[thread]['tags']:
            evidence=set(topics[tag['topic']]['sources'])&local;tp=len(expected&evidence)
            values[tag['topic']]=2*tp/(len(expected)+len(evidence)) if expected or evidence else 0.
        recall=recalled/len(expected);best=max(values.values(),default=0.)
        error=abs(best-row['best_single_phrase_support']['f1']);maximum_error=max(maximum_error,error)
        assert math.isclose(best,row['best_single_phrase_support']['f1'],rel_tol=0,abs_tol=1e-12)
        assert math.isclose(recall,row['any_phrase_evidence_recall'],rel_tol=0,abs_tol=1e-12)
        assert row['support_sources']==len(expected) and row['sources_with_any_selected_phrase']==recalled
        assert row['best_single_phrase'] in values and math.isclose(values[row['best_single_phrase']],best,abs_tol=1e-12)
        name={atom for term in counts(subject['subject']) for atom in term.split()}
        selected={atom for tag in threads[thread]['tags'] for atom in topics[tag['topic']]['phrase'].split()}
        hit=bool(name&selected);assert row['subject_name_atom_hit']==hit
        summary[subject['importance']].append((recall,best,hit,recalled));checked+=1
    for importance,rows in summary.items():
        actual=a['summary'][importance];assert actual['subjects']==len(rows)
        assert actual['subjects_with_no_literal_evidence']==sum(not row[3] for row in rows)
        assert actual['subject_name_atom_hits']==sum(row[2] for row in rows)
        assert math.isclose(actual['mean_any_phrase_evidence_recall'],sum(row[0] for row in rows)/len(rows),abs_tol=1e-12)
        assert math.isclose(actual['mean_best_single_phrase_support_f1'],sum(row[1] for row in rows)/len(rows),abs_tol=1e-12)
    return {'stable_repeat':True,'admitted_sources_checked':len(sources),'reviewed_subjects_checked':checked,
        'maximum_f1_error':maximum_error,'scope':'source hashes/literal memberships and original-review support recall, maximum F1 and summary arithmetic',
        'shared_dependency':'sklearn static English stoplist through independent scanner',
        'rankings_lifecycle_native_semantic_quality_independently_verified':False,'independent_human_gold':False}


def audit(packet_path,review_path,first,repeat):
    packet=json.loads(packet_path.read_text());review=json.loads(review_path.read_text())
    a=json.loads(first.read_text());b=json.loads(repeat.read_text());a.pop('runtime');b.pop('runtime');assert a==b
    assert hashlib.sha256(packet_path.read_bytes()).hexdigest()==a['packet_sha256']==review['packet_sha256']
    assert hashlib.sha256(review_path.read_bytes()).hexdigest()==a['review_sha256']
    return audit_data(packet,review,a)


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['packet','review','first','repeat']:parser.add_argument('--'+name,type=Path,required=True)
    args=parser.parse_args();print(json.dumps(audit(args.packet,args.review,args.first,args.repeat),sort_keys=True))
