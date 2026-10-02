"""Source-grounded phrase subjects; research-only flat topic candidate."""
from collections import Counter,defaultdict
from dataclasses import dataclass
from enum import StrEnum
import hashlib
import math
import numpy as np
from subject_labels import ranked_terms
from topic_features import terms
from phrase_selection import Selection,select_source_coverage,PHATIC_ATOMS
from topic_background import EnglishBackground


class Support(StrEnum):
    RECURRING='cross_thread_and_distinct_input'
    PROVISIONAL='provisional'


@dataclass(frozen=True)
class PhraseSource:
    id: str
    thread: str
    provider: str
    text: str


def discover(sources,namespace,maximum_tags=8,selection=Selection.WHOLE_THREAD,background=None):
    if type(maximum_tags) is not int or not 1<=maximum_tags<=8:raise ValueError('bounded thread tag selection required')
    if not isinstance(selection,Selection):raise ValueError('typed phrase selection policy required')
    if selection==Selection.BACKGROUND_COVERAGE:
        if not isinstance(background,EnglishBackground):raise ValueError('explicit English background frame required')
    elif background is not None:raise ValueError('background supplied for another policy')
    threads=defaultdict(list);tokens={};providers={}
    for source in sources:
        if source.thread in providers and providers[source.thread]!=source.provider:raise ValueError('thread provider scope differs')
        providers[source.thread]=source.provider;threads[source.thread].append(source)
        tokens[source.id]=Counter(terms(source.text))
    # Exact duplicates within a thread retain evidence identities but only one
    # lexical vote. Across-thread recurrence also needs two distinct inputs.
    classes={};inputs=defaultdict(set);term_threads=defaultdict(set)
    for thread,rows in sorted(threads.items()):
        pool=Counter();seen=set()
        for source in rows:
            if source.text not in seen:pool.update(tokens[source.id]);seen.add(source.text)
            for term in tokens[source.id]:
                inputs[term].add(source.text);term_threads[term].add(thread)
        classes[thread]=pool
    total=Counter()
    for pool in classes.values():total.update(pool)
    vocabulary=sorted(sorted(total,key=lambda term:(-total[term],term))[:8192])
    keep=set(vocabulary);weighted={thread:{term:1+math.log(count) for term,count in pool.items() if term in keep}
                                   for thread,pool in classes.items()}
    frequency=Counter()
    for pool in weighted.values():frequency.update(pool)
    lengths={thread:sum(pool.values()) for thread,pool in weighted.items()}
    mean=sum(lengths.values())/len(lengths) if lengths else 0.
    topic_map={};thread_rows=[];memberships={source.id:[] for source in sources}
    factors=background.factors(vocabulary) if background is not None else {}
    for thread,rows in sorted(threads.items()):
        pool=weighted[thread];values=[]
        for term in vocabulary:
            df=len(term_threads[term]);allowed=any(c.isalpha() for c in term) and (len(threads)<3 or df<=.4*len(threads))
            score=(pool.get(term,0)/max(lengths[thread],1)*math.log1p(mean/max(frequency[term],1))
                   *(math.log((1+len(threads))/(1+df))+1)) if allowed else 0.
            values.append(score*factors.get(term,1.))
        if selection!=Selection.WHOLE_THREAD:selected=select_source_coverage(values,vocabulary,rows,tokens,maximum_tags,selection)
        else:selected=ranked_terms(np.array(values),vocabulary,diverse=True,limit=maximum_tags) if vocabulary else []
        tags=[]
        for candidate in selected:
            term=candidate['term'];identity='phrase-'+hashlib.sha256((namespace+'\0'+term).encode()).hexdigest()
            support=Support.RECURRING if len(term_threads[term])>=2 and len(inputs[term])>=2 else Support.PROVISIONAL
            topic=topic_map.setdefault(identity,{'id':identity,'phrase':term,'support':support,
                'corpus_thread_support':len(term_threads[term]),'corpus_distinct_inputs':len(inputs[term]),'sources':set(),'threads':set()})
            topic['threads'].add(thread)
            tag={'topic':identity,'score':candidate['score']};tags.append(tag)
            for source in rows:
                if term in tokens[source.id]:topic['sources'].add(source.id);memberships[source.id].append(tag.copy())
        thread_rows.append({'thread':thread,'provider':providers[thread],'sources':sorted(source.id for source in rows),'tags':tags})
    topics=[]
    for identity,topic in sorted(topic_map.items()):
        topics.append({**topic,'sources':sorted(topic['sources']),'threads':sorted(topic['threads'])})
    result={'schema_version':1,'representation':'flat_phrase_subjects','namespace':namespace,'topics':topics,
        'threads':thread_rows,'memberships':memberships,'score_kind':'thread_sublinear_ctfidf_times_thread_idf',
        'parameters':{'maximum_thread_tags':maximum_tags,'maximum_vocabulary':8192,'maximum_thread_df_fraction':.4,
            'background_filter_warmup_threads':3,'recurring_minimum_threads':2,'recurring_minimum_distinct_inputs':2,
            'duplicates':'one lexical vote per exact input per thread; original evidence retained'},
        'diagnostics':{'sources':len(sources),'threads':len(threads),'topics':len(topics),'vocabulary':len(vocabulary),
            'recurring_topics':sum(topic['support']==Support.RECURRING for topic in topics),
            'provisional_topics':sum(topic['support']==Support.PROVISIONAL for topic in topics),
            'untagged_sources':sum(not tags for tags in memberships.values()),
            'multi_tag_sources':sum(len(tags)>1 for tags in memberships.values()),
            'untagged_threads':sum(not thread['tags'] for thread in thread_rows)}}
    if selection!=Selection.WHOLE_THREAD:
        result['parameters'].update({'selection':selection,'source_unit_candidates':2,'coverage_utility_weight':.5,
            'coverage_units':'distinct exact inputs; supports top-two eligible local candidates'})
    if selection==Selection.PHATIC_COVERAGE:
        result['parameters']['candidate_atom_exclusions']=sorted(PHATIC_ATOMS)
    if selection==Selection.BACKGROUND_COVERAGE:
        result['parameters']['background']={'asset_sha256':background.asset_sha256,'language':'en','wordlist':'large',
            'factor':'mean_atom_clamped_negative_log10_probability_divided_by_9','minimum_information':1.,'maximum_information':9.,'probability_floor':1e-9}
        result['score_kind']='thread_sublinear_ctfidf_times_thread_idf_times_background_information'
    return result
