"""No producer imports: flat source unions and scalar label-score audit."""
from collections import Counter
import argparse
import json
import math
from pathlib import Path
from sklearn.feature_extraction.text import CountVectorizer
from topic_capacity_audit import counts


def audit(source_baseline,first,repeat):
    original=json.loads(source_baseline.read_text())['baseline_checkpoint']['records']
    a=json.loads(first.read_text());b=json.loads(repeat.read_text())
    a.pop('runtime');b.pop('runtime');assert a==b
    sources={row['id']:row for row in original};words={sid:counts(row['text']) for sid,row in sources.items()}
    # Share sklearn's max-feature selection, not producer tokenization, class
    # pooling or scoring. Source term multiplicities are independently counted.
    vectorizer=CountVectorizer(analyzer=lambda text:counts(text).elements(),max_features=8192)
    vectorizer.fit_transform(row['text'] for row in original)
    vocabulary=set(vectorizer.get_feature_names_out())
    df=Counter(term for row in words.values() for term in row if term in vocabulary)
    checked=topics_checked=0;maximum_error=0.
    for name in ['flat_every16','flat_every256','flat_final']:
        for direction in ['forward','reverse']:
            snapshot=a['arms'][name][direction]
            assert snapshot==a['arms']['flat_final'][direction]
            assert snapshot['representation']=='flat_multi_label' and 'nodes' not in snapshot and 'root' not in snapshot
            assert set(snapshot['versions'])==set(snapshot['memberships'])==set(sources)
            topics=snapshot['topics'];ids={topic['id'] for topic in topics};assert len(ids)==len(topics)
            class_counts=[]
            for topic in topics:
                assert not {'parent','children','ancestor_tags'}&topic.keys()
                members={sid for sid,tags in snapshot['memberships'].items() if any(t['topic']==topic['id'] for t in tags)}
                assert members==set(topic['sources']) and len(members)==len(topic['sources'])
                assert set(topic['threads'])=={sources[sid]['thread'] for sid in members}
                pool=Counter()
                for sid in members:pool.update({term:n for term,n in words[sid].items() if term in vocabulary})
                class_counts.append(pool)
            total=Counter()
            for pool in class_counts:total.update(pool)
            average=sum(sum(pool.values()) for pool in class_counts)/len(class_counts)
            for topic,pool in zip(topics,class_counts):
                topics_checked+=1;members=set(topic['sources']);length=sum(pool.values())
                assert topic['label_support']==('supported' if len(topic['labels'])>=2 else 'weak')
                for label in topic['labels']:
                    term=label['term'];support=sum(term in words[sid] for sid in members)
                    assert term in vocabulary and support>=2 and support==label['source_support']
                    assert df[term]<=.4*len(sources) and any(c.isalpha() for c in term)
                    score=pool[term]/max(length,1)*math.log1p(average/max(total[term],1))*(math.log((1+len(sources))/(1+df[term]))+1)
                    error=abs(score-label['score']);maximum_error=max(maximum_error,error)
                    assert math.isfinite(label['score']) and math.isclose(score,label['score'],rel_tol=1e-12,abs_tol=1e-12)
                    checked+=1
            for sid,tags in snapshot['memberships'].items():
                assert len(tags)<=3 and len({t['topic'] for t in tags})==len(tags)
                assert all(t['topic'] in ids and math.isfinite(t['score']) and .55<=t['score']<=1+1e-12 for t in tags)
            threads=snapshot['threads'];assert len({row['thread'] for row in threads})==len(threads)
            assert {row['thread'] for row in threads}=={row['thread'] for row in sources.values()}
            for thread in threads:
                members={sid for sid,row in sources.items() if row['thread']==thread['thread']}
                assert set(thread['sources'])==members and len(thread['sources'])==len(members)
                assert set(thread['tags'])=={tag['topic'] for sid in members for tag in snapshot['memberships'][sid]}
                assert all(sources[sid]['provider']==thread['provider'] for sid in members)
                assert 'ancestor_tags' not in thread
    return {'stable_repeat':True,'schedule_final_exact':True,'flat_snapshots_checked':6,
        'topics_checked':topics_checked,'label_scores_checked':checked,'maximum_label_score_error':maximum_error,
        'shared_dependencies':'sklearn English stoplist and max-feature vocabulary selection',
        'scope':'original-source unions, provider/thread tags, flat scalar label scores, repeat/schedule equality',
        'routing_winners_label_rank_semantic_quality_verified':False}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['source_baseline','first','repeat']:parser.add_argument('--'+name.replace('_','-'),type=Path,required=True)
    args=parser.parse_args();print(json.dumps(audit(args.source_baseline,args.first,args.repeat),sort_keys=True))
