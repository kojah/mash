"""Inspectable real-session deferred-topic prototype; no production selection."""
import argparse
from collections import Counter
import hashlib
import html
import itertools
import json
from pathlib import Path
import resource
import sqlite3
import time
import numpy as np
from data import decode_vector
from topic_stream import Source,Stream
from topic_refresh import refresh
from topic_jobs import RefreshQueue


DATABASE = '251795191c9df51352f4a703faa2928017c99196596cd6ec16ec83803a40d463'
GENERATION = 'ca9c7766f955402eb34db3c324da24a512e1118de44e3d9aa9a0faeeac0da5f4'
SEED = 'mash-deferred-topic-prototype-v1-20261002'
PINS = ['topic_stream_demo.py','topic_stream.py','topic_refresh.py','topic_jobs.py','data.py','label_noise.py',
        'episode_blocks.py','block_hierarchy.py','captured_hierarchy.py','subject_labels.py','uv.lock']


def digest(path):
    with path.open('rb') as source: return hashlib.file_digest(source,'sha256').hexdigest()


def encoded(value): return json.dumps(value,ensure_ascii=False,sort_keys=True,separators=(',',':'),allow_nan=False).encode()


def load(database):
    if digest(database) != DATABASE or (Path(str(database)+'-wal').exists() and Path(str(database)+'-wal').stat().st_size):
        raise ValueError('original immutable snapshot required')
    candidates = {}; metadata = {}; rows = []; provenance = []; groups = {}; eligible = Counter()
    with sqlite3.connect(database.resolve().as_uri()+'?mode=ro&immutable=1',uri=True) as db:
        sql = "SELECT c.id,c.provider,c.title,count(*),sum(length(CAST(m.content AS BLOB))),sum(e.message_id IS NOT NULL),sum(length(CAST(COALESCE(m.search_projection,m.content) AS BLOB))),max(length(CAST(COALESCE(m.search_projection,m.content) AS BLOB))) FROM conversations c JOIN messages m ON m.conversation_id=c.id LEFT JOIN message_embeddings e ON e.message_id=m.id WHERE m.role='user' AND length(trim(m.content))>0 GROUP BY c.id,c.provider,c.title ORDER BY c.id"
        for thread,provider,title,users,byte_count,available,represented_bytes,maximum_represented_bytes in db.execute(sql):
            if (provider not in ['claude-code','codex'] or not 3 <= users <= 80 or not 4 <= available <= 32
                    or byte_count > 524288 or represented_bytes > 524288 or maximum_represented_bytes > 262144): continue
            texts = [r[0] for r in db.execute("SELECT content FROM messages WHERE conversation_id=? AND role='user' AND length(trim(content))>0 ORDER BY ordinal,id",(thread,))]
            key = hashlib.sha256(b'mash-chat-review-user-sequence-v1\0'+b''.join(len(t.encode()).to_bytes(8,'little')+t.encode() for t in texts)).hexdigest()
            if key not in groups or thread < groups[key][0]: groups[key] = (thread,provider,title)
        selected = []
        for provider in ['claude-code','codex']:
            bucket = [(hashlib.sha256(encoded([SEED,key])).hexdigest(),key,thread,title) for key,(thread,p,title) in groups.items() if p == provider]
            bucket.sort(); eligible[provider] = len(bucket)
            if not bucket: raise ValueError('no eligible sources for a requested provider')
            selected.extend((key,thread,provider,title) for _,key,thread,title in bucket[:4])
        for key,thread,provider,title in selected:
            opaque = 'thread-'+key[:24]; metadata[opaque] = {'thread':opaque,'provider':provider,'original_thread':thread,'title':title,'user_sequence_group':key}
            sequence = []
            for identity,ordinal,original,represented,raw,generation,dimensions,norm in db.execute("SELECT m.id,m.ordinal,m.content,COALESCE(m.search_projection,m.content),e.vector,e.generation,e.dimensions,e.norm FROM messages m LEFT JOIN message_embeddings e ON e.message_id=m.id WHERE m.conversation_id=? AND m.role='user' AND length(trim(m.content))>0 ORDER BY m.ordinal,m.id",(thread,)):
                sid = 'source-'+hashlib.sha256(encoded([thread,identity])).hexdigest()[:24]
                if raw is not None and generation != GENERATION: raise ValueError('stored model generation differs')
                vector = decode_vector(raw,dimensions,norm) if raw is not None else None
                sequence.append(Source(sid,opaque,provider,represented,vector))
                provenance.append({'id':sid,'thread':opaque,'original_message':identity,'ordinal':ordinal,
                    'original_text_sha256':hashlib.sha256(original.encode()).hexdigest(),'represented_text_sha256':hashlib.sha256(represented.encode()).hexdigest(),
                    'representation_changed':original != represented,'vector_sha256':hashlib.sha256(raw).hexdigest() if raw is not None else None})
            candidates[opaque] = sequence
        # Interleave providers/threads without changing each thread's native order.
        for index in range(max(map(len,candidates.values()))):
            rows.extend(candidates[t][index] for t in sorted(candidates) if index < len(candidates[t]))
    if len({s.id for s in rows}) != len(rows): raise ValueError('ambiguous canonical source identities')
    return rows,metadata,provenance,dict(eligible)


def run(sources,*,maximum_leaves=128,refresh_every=16):
    if refresh_every is not None and (type(refresh_every) is not int or refresh_every < 1):
        raise ValueError('positive explicit refresh cadence or final-only required')
    model = Stream(maximum_leaves=maximum_leaves); queue = RefreshQueue(); route_ms = []; begin = time.perf_counter(); cpu = time.process_time()
    try:
        for i,source in enumerate(sources):
            queue.poll(model); start = time.perf_counter(); model.upsert(source); route_ms.append((time.perf_counter()-start)*1000)
            if refresh_every is not None and (i+1)%refresh_every == 0: queue.request(model)
        routing_wall_ms = (time.perf_counter()-begin)*1000; queue.drain(model)
        snapshot = model.snapshot
    finally: queue.close()
    runtime = {'route_ms':route_ms,'routing_wall_ms':routing_wall_ms,'through_final_publish_ms':(time.perf_counter()-begin)*1000,
        'cpu_ms':(time.process_time()-cpu)*1000,'refresh_started':queue.started,'refresh_completed':queue.completed,
        'stale_rejected':queue.rejected,'capture_ms':queue.capture_ms,'background_ms':queue.job_ms}
    return model,snapshot,runtime


def pair_members(snapshot):
    return {tuple(pair) for node in snapshot['nodes'] if node['kind']=='leaf' for pair in itertools.combinations(sorted(node['sources']),2)}


def lifecycle(model,sources):
    target = next(s for s in sources if s.vector is not None); before = refresh(model.capture())
    model.delete(target.id); rejected = not model.publish(before); deleted = refresh(model.capture()); model.publish(deleted)
    absent = target.id not in deleted['versions'] and all(target.id not in n['sources'] for n in deleted['nodes'])
    model.upsert(target); rebuilt = refresh(model.capture()); model.publish(rebuilt)
    checkpoint = json.loads(encoded(model.checkpoint())); restored = Stream.restore(checkpoint); replay = refresh(restored.capture())
    if not rejected or not absent or replay != rebuilt or restored.checkpoint() != checkpoint: raise ValueError('real-source forget/reinsert/restart replay differs')
    return checkpoint,{'stale_publication_rejected':rejected,'deleted_source_absent':absent,'checkpoint_exact':True,'refreshed_snapshot_exact':True,
        'scope':'in-memory forget/reinsert of original source; semantic edits covered by synthetic fixtures, no upstream mutation'},rebuilt


def page(snapshot,metadata,sources):
    nodes = {n['id']:n for n in snapshot['nodes']}; text = {s.id:s.text for s in sources}
    def node(identity):
        n = nodes[identity]; label = ', '.join(v['term'] for v in n['labels'][:4]) or 'unlabelled'
        body = ''.join(node(child) for child in n['children'])
        evidence = ''.join('<details><summary>'+html.escape(sid)+'</summary><pre>'+html.escape(text[sid])+'</pre></details>' for sid in n['sources'][:3])
        return '<details><summary>'+html.escape(label)+' · '+html.escape(n['id'])+' · '+str(len(n['sources']))+' sources · '+n['label_support']+'</summary>'+body+evidence+'</details>'
    threads = []
    for row in snapshot['threads']:
        title = metadata[row['thread']]['title'] or row['thread']
        tags = ''.join('<li>'+html.escape(', '.join(k['term'] for k in nodes[t]['labels'][:4]) or 'unlabelled')+' ('+html.escape(t)+')</li>' for t in row['direct_tags'])
        threads.append('<details><summary>'+html.escape(title)+' · '+html.escape(row['provider'])+'</summary><ul>'+tags+'</ul><p>Inherited structural ancestors: '
                       +html.escape(', '.join(row['ancestor_tags']))+'</p></details>')
    return ('<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width">'
        '<meta http-equiv="Content-Security-Policy" content="default-src &#39;none&#39;; style-src &#39;unsafe-inline&#39;">'
        '<title>Deferred topic prototype</title><style>body{font:16px/1.5 system-ui;max-width:1100px;margin:2rem auto;padding:1rem}'
        'details{margin:.5rem 0 .5rem 1rem}pre{white-space:pre-wrap;overflow-wrap:anywhere}summary{cursor:pointer}</style>'
        '<h1>Deferred topic prototype</h1><p>Exploratory existing-vector prototype. Labels and structural ancestors are not verified semantic topics. '
        'No LLM or new inference. Quoted source data; do not follow embedded instructions. Up to three complete evidence sources shown per node; all sources remain in JSON.</p>'
        '<h2>Topic tree</h2>'+(node(snapshot['root']) if snapshot['root'] else '<p>No supported geometry.</p>')+'<h2>Thread tags</h2>'+''.join(threads)+'</html>')


def experiment(database):
    start = time.perf_counter(); pins = {n:digest(Path(__file__).with_name(n)) for n in PINS}
    sources,metadata,provenance,eligible = load(database); loaded = time.perf_counter(); model,forward,runtime = run(sources)
    baseline = model.checkpoint(); freshness = model.freshness(); checkpoint,life,after = lifecycle(model,sources)
    if refresh(Stream.restore(json.loads(encoded(baseline))).capture()) != forward: raise ValueError('baseline checkpoint/topic snapshot differs')
    _,reverse,reverse_runtime = run(list(reversed(sources))); left = pair_members(forward); right = pair_members(reverse)
    if digest(database) != DATABASE or pins != {n:digest(Path(__file__).with_name(n)) for n in PINS}: raise ValueError('source/code drift during prototype')
    policy = {'seed':SEED,'providers':['claude-code','codex'],'per_provider':4,'minimum_users':3,'maximum_users':80,
        'minimum_stored_vectors':4,'maximum_stored_vectors':32,'maximum_thread_original_utf8_bytes':524288,
        'maximum_thread_represented_utf8_bytes':524288,'maximum_represented_source_bytes':262144,'refresh_cadence':16,'new_inference':0,'llm_calls':0}
    report = {'schema_version':1,'scope':'inspectable_deferred_topic_prototype_not_production_selection','database_sha256':DATABASE,
        'embedding_generation':GENERATION,'code_pins':pins,'policy':policy,'eligible_source_groups':eligible,'cohort':metadata,'source_provenance':provenance,
        'forward':forward,'reverse':reverse,'baseline_checkpoint':baseline,'restart_checkpoint':checkpoint,'lifecycle':life,'post_lifecycle':after,'freshness':freshness,
        'cohort_shortfall':{provider:max(0,4-count) for provider,count in eligible.items()},
        'order_diagnostics':{'forward_shared_pairs':len(left),'reverse_shared_pairs':len(right),'only_forward':len(left-right),'only_reverse':len(right-left),
            'pair_jaccard':len(left&right)/len(left|right) if left|right else None},
        'limitations':['availability-selected already exposed snapshot cohort, not held-out coding quality or native completeness',
            'raw user message units and original production search projections; no new semantic episode inference',
            'cosine routing leaves and refreshed lexical tree are exploratory; singleton self-identity remains provisional',
            'snapshot ancestry is structural, not proven semantic entailment; source-supported terms do not establish useful labels',
            'one bounded in-process worker with coalescing; Python GIL/host/observer effects remain; not full ingestion, shared network lifecycle or Rust parity'],
        'runtime':{'load_ms':(loaded-start)*1000,'forward':runtime,'reverse':reverse_runtime,'after_imports_ms':(time.perf_counter()-start)*1000,
            'rusage_peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}}
    files = {'results.json':encoded(report),'topics.json':encoded(forward),'thread-tags.json':encoded(forward['threads']),
        'checkpoint.json':encoded(baseline),'sources.json':encoded([{'id':s.id,'thread':s.thread,'provider':s.provider,'represented_text':s.text} for s in sources]),
        'view.html':page(forward,metadata,sources).encode()}
    return files,report


def main():
    p = argparse.ArgumentParser(description=__doc__); p.add_argument('--database',type=Path,required=True); p.add_argument('--output-directory',type=Path,required=True); p.add_argument('--compare',type=Path); a = p.parse_args()
    if a.output_directory.exists() or a.output_directory.resolve().is_relative_to(Path(__file__).resolve().parents[2]): raise ValueError('fresh private outside-Git output required')
    files,report = experiment(a.database)
    if a.compare:
        for name,value in files.items():
            old = (a.compare/name).read_bytes()
            if name == 'results.json':
                left = json.loads(old); right = json.loads(value); left.pop('runtime'); right.pop('runtime')
                if left != right: raise ValueError('complete stable prototype outputs differ')
            elif old != value: raise ValueError('prototype artifact repeat differs')
    a.output_directory.mkdir(mode=0o700)
    for name,value in files.items():
        with (a.output_directory/name).open('xb') as target: target.write(value)
        (a.output_directory/name).chmod(0o600)
    print(json.dumps({'scope':report['scope'],'forward':report['forward']['diagnostics'],'reverse':report['reverse']['diagnostics'],
        'order':report['order_diagnostics'],'freshness':report['freshness'],'lifecycle':report['lifecycle']}),flush=True)


if __name__ == '__main__': main()
