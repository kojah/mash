"""Deferred flat multi-label topics; no hierarchy, ancestors or inference."""
import math
from topic_features import assignments,lexical_counts,labels_for_sets


def refresh_flat(capture,threshold=.55,relative=.8,maximum_tags=3):
    if (isinstance(threshold,bool) or not math.isfinite(threshold) or not 0 < threshold <= 1
            or isinstance(relative,bool) or not math.isfinite(relative) or not 0 < relative <= 1
            or type(maximum_tags) is not int or not 1 <= maximum_tags <= 3):
        raise ValueError('finite bounded membership parameters required')
    records=capture.records;ids=capture.leaf_ids
    ready,memberships,source_sets=assignments(capture,threshold,relative,maximum_tags)
    vocabulary,counts,df,idf=lexical_counts(records,ready)
    # Class statistics contain only flat topics. Historical ancestor classes
    # must not contribute duplicated term counts to the flat label weights.
    labels=labels_for_sets(counts,vocabulary,ready,df,idf,source_sets)
    topics=[]
    for identity,members,keywords in zip(ids,source_sets,labels):
        topics.append({'id':'leaf-'+str(identity),'labels':keywords,
            'label_support':'supported' if len(keywords)>=2 else 'weak',
            'sources':[records[i].source.id for i in sorted(members)],
            'threads':sorted({records[i].source.thread for i in members})})
    threads={}
    for record in records:
        source=record.source
        thread=threads.setdefault(source.thread,{'thread':source.thread,'provider':source.provider,'sources':[],'tags':set()})
        if thread['provider']!=source.provider:raise ValueError('thread provider scope differs')
        thread['sources'].append(source.id)
        thread['tags'].update(tag['topic'] for tag in memberships[source.id])
    for thread in threads.values():
        thread['tags']=sorted(thread['tags']);thread['sources'].sort()
    return {'schema_version':1,'representation':'flat_multi_label','revision':capture.revision,
        'versions':{r.source.id:r.version for r in records},'topics':topics,'memberships':memberships,
        'threads':[threads[key] for key in sorted(threads)],
        'parameters':{'threshold':threshold,'relative':relative,'maximum_tags':maximum_tags,
            'maximum_vocabulary':8192,'minimum_label_source_support':2,'maximum_label_df_fraction':.4,
            'own_centroid':'leave_one_out_when_nonsingleton','label_classes':'flat_topics_only'},
        'diagnostics':{'sources':len(records),'stored_vectors':len(ready),'missing_vectors':len(records)-len(ready),
            'untagged_ready_sources':sum(not memberships[records[i].source.id] for i in ready),
            'topics':len(ids),'multi_tag_sources':sum(len(tags)>1 for tags in memberships.values()),
            'weak_topics':sum(topic['label_support']=='weak' for topic in topics),'vocabulary':len(vocabulary),
            'singleton_topics':sum(len(topic['sources'])==1 for topic in topics),
            'empty_topics':sum(not topic['sources'] for topic in topics)}}
