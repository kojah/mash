"""Bounded deferred prototype across frozen 128-thread selection; no inference."""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import resource
import sqlite3
import time
import numpy as np
from data import decode_vector
from topic_stream import Source,Stream
from topic_refresh import refresh
from topic_stream_demo import DATABASE,GENERATION,run,lifecycle,page,pair_members,encoded


SELECTION='270fdf50405136931791905b8555f5e4d7a35cb5a5fd935c4a201fd72b618167'
PINS=['topic_cohort_demo.py','topic_stream_demo.py','topic_stream.py','topic_refresh.py','topic_jobs.py','data.py',
      'episode_blocks.py','block_hierarchy.py','captured_hierarchy.py','subject_labels.py','label_noise.py','uv.lock']


def digest(path):
    with path.open('rb') as file:return hashlib.file_digest(file,'sha256').hexdigest()


def load(database,selection):
    if digest(database)!=DATABASE or digest(selection)!=SELECTION:raise ValueError('original frozen database/selection required')
    if Path(str(database)+'-wal').exists() and Path(str(database)+'-wal').stat().st_size:
        raise ValueError('immutable source must not have live WAL')
    selected=json.loads(selection.read_text())['records'];sequences={};metadata={};provenance=[];excluded=[];ledger=hashlib.sha256()
    with sqlite3.connect(database.resolve().as_uri()+'?mode=ro&immutable=1',uri=True) as db:
        for thread,expected in sorted(selected.items()):
            provider,title=db.execute('SELECT provider,title FROM conversations WHERE id=?',(thread,)).fetchone()
            if provider!=expected['provider']:raise ValueError('selected provider differs')
            opaque='thread-'+hashlib.sha256(thread.encode()).hexdigest()[:24]
            metadata[opaque]={'thread':opaque,'original_thread':thread,'provider':provider,'title':title}
            sequence=[];unique={};original_users=0
            for identity,ordinal,original,text,raw,generation,dimensions,norm in db.execute(
                    "SELECT m.id,m.ordinal,m.content,COALESCE(m.search_projection,m.content),e.vector,e.generation,e.dimensions,e.norm FROM messages m LEFT JOIN message_embeddings e ON e.message_id=m.id WHERE m.conversation_id=? AND m.role='user' ORDER BY m.ordinal,m.id",(thread,)):
                original_users+=1;sid='source-'+hashlib.sha256(encoded([thread,identity])).hexdigest()[:24]
                entry={'id':sid,'thread':opaque,'original_message':identity,'ordinal':ordinal,
                       'original_text_sha256':hashlib.sha256(original.encode()).hexdigest(),
                       'represented_text_sha256':hashlib.sha256(text.encode()).hexdigest(),
                       'vector_sha256':hashlib.sha256(raw).hexdigest() if raw is not None else None,
                       'status':'admitted' if text else 'excluded_empty_projection'}
                value=encoded(entry);ledger.update(len(value).to_bytes(8,'little'));ledger.update(value)
                if not text:
                    if raw is not None:raise ValueError('excluded source unexpectedly has geometry')
                    excluded.append(entry);continue
                if raw is None or generation!=GENERATION:raise ValueError('admitted source lacks compatible stored geometry')
                vector=decode_vector(raw,dimensions,norm)
                sequence.append(Source(sid,opaque,provider,text,vector));provenance.append(entry)
                unique.setdefault(entry['represented_text_sha256'],(identity,ordinal,entry['vector_sha256'],vector))
            if original_users!=expected['user_messages']:raise ValueError('original user-record count differs')
            if set(unique)!={m['input_sha256'] for m in expected['messages']}:raise ValueError('selected exact inputs differ')
            for message in expected['messages']:
                identity,ordinal,vector_sha,vector=unique[message['input_sha256']]
                if (identity,ordinal,vector_sha)!=(message['identity'],message['ordinal'],message['stored_vector_sha256']):
                    raise ValueError('selected original message ownership differs')
                if not np.array_equal(vector,np.array(message['direction'])):raise ValueError('selected stored direction differs')
            sequences[opaque]=sequence
    sources=[sequences[t][i] for i in range(max(map(len,sequences.values()))) for t in sorted(sequences) if i<len(sequences[t])]
    if len(metadata)!=128 or len(sources)!=1850 or len(excluded)!=7038 or len({s.id for s in sources})!=1850:
        raise ValueError('complete original cohort counts differ')
    return sources,metadata,provenance,excluded,ledger.hexdigest()


def experiment(database,selection):
    pins={name:digest(Path(__file__).with_name(name)) for name in PINS};start=time.perf_counter();cpu=time.process_time()
    sources,metadata,provenance,excluded,ledger=load(database,selection);loaded=time.perf_counter()
    model,forward,fr=run(sources);baseline=model.checkpoint();states=Counter(r.state.value for r in model.records.values())
    freshness=model.freshness()
    if refresh(Stream.restore(json.loads(encoded(baseline))).capture())!=forward:raise ValueError('baseline restart differs')
    checkpoint,life,after=lifecycle(model,sources)
    reverse_model,reverse,rr=run(list(reversed(sources)));reverse_states=Counter(r.state.value for r in reverse_model.records.values())
    left=pair_members(forward);right=pair_members(reverse)
    if digest(database)!=DATABASE or digest(selection)!=SELECTION or pins!={n:digest(Path(__file__).with_name(n)) for n in PINS}:
        raise ValueError('source/code drift during complete cohort experiment')
    result={'schema_version':1,'scope':'frozen_128_thread_bounded_deferred_prototype_not_selection','database_sha256':DATABASE,
            'selection_sha256':SELECTION,'embedding_generation':GENERATION,'code_pins':pins,'source_ledger_sha256':ledger,
            'cohort':metadata,'source_provenance':provenance,'excluded_source_provenance':excluded,
            'policy':{'arrival':'round-robin admitted messages per selected thread; reverse simulated arrival',
                      'source_occurrences':1850,'deduplicated_selected_inputs':1812,'excluded_canonical_user_records':7038,
                      'maximum_sources':2048,'maximum_leaves':128,'maximum_text_bytes':8388608,'threshold':.55,'new_inference':0},
            'forward':forward,'reverse':reverse,'baseline_checkpoint':baseline,'restart_checkpoint':checkpoint,
            'lifecycle':life,'post_lifecycle':after,'freshness':freshness,'routing_states':dict(states),'reverse_routing_states':dict(reverse_states),
            'order':{'forward_pairs':len(left),'reverse_pairs':len(right),'only_forward':len(left-right),'only_reverse':len(right-left),
                     'pair_jaccard':len(left&right)/len(left|right) if left|right else None},
            'semantic_quality_verified':False,'production_selected':False,
            'runtime':{'load_ms':(loaded-start)*1000,'forward':fr,'reverse':rr,'total_ms':(time.perf_counter()-start)*1000,
                       'cpu_ms':(time.process_time()-cpu)*1000,'rusage_peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}}
    files={'results.json':encoded(result),'topics.json':encoded(forward),'thread-tags.json':encoded(forward['threads']),
           'checkpoint.json':encoded(baseline),'sources.json':encoded([{'id':s.id,'thread':s.thread,'provider':s.provider,'represented_text':s.text} for s in sources]),
           'view.html':page(forward,metadata,sources).encode()}
    return result,files


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['database','selection','output-directory']:parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--compare',type=Path);args=parser.parse_args()
    if args.output_directory.exists() or args.output_directory.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh private outside-Git output required')
    result,files=experiment(args.database,args.selection)
    if args.compare:
        for name,value in files.items():
            prior=(args.compare/name).read_bytes()
            if name=='results.json':
                old=json.loads(prior);new=json.loads(value);old.pop('runtime');new.pop('runtime')
                if old!=new:raise ValueError('stable complete cohort results differ')
            elif prior!=value:raise ValueError('stable complete cohort artifact differs')
    args.output_directory.mkdir(mode=0o700)
    for name,value in files.items():
        path=args.output_directory/name
        with path.open('xb') as file:file.write(value)
        path.chmod(0o600)
    print(json.dumps({'forward':result['forward']['diagnostics'],'reverse':result['reverse']['diagnostics'],
                      'routing_states':result['routing_states'],'reverse_routing_states':result['reverse_routing_states'],
                      'order':result['order'],'lifecycle':result['lifecycle']}))
