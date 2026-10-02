"""Independent artifact/source and structural checks; not semantic-quality validation."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import sqlite3
import struct


def audit(directory,database):
    report=json.loads((directory/'results.json').read_text())
    snapshot=json.loads((directory/'topics.json').read_text())
    sources=json.loads((directory/'sources.json').read_text())
    checkpoint=json.loads((directory/'checkpoint.json').read_text())
    assert snapshot==report['forward'] and checkpoint==report['baseline_checkpoint']
    assert json.loads((directory/'thread-tags.json').read_text())==snapshot['threads']
    with database.open('rb') as file:
        assert hashlib.file_digest(file,'sha256').hexdigest()==report['database_sha256']
    by_source={s['id']:s for s in sources}
    assert len(by_source)==len(sources)
    records={r['id']:r for r in checkpoint['records']}
    assert set(records)==set(by_source)==set(snapshot['versions'])
    assert {p['id'] for p in report['source_provenance']}==set(by_source)
    assert len(report['source_provenance'])==len(by_source)
    canonical=set()
    vector_checks=0
    with sqlite3.connect(database.resolve().as_uri()+'?mode=ro&immutable=1',uri=True) as db:
        for p in report['source_provenance']:
            source=by_source[p['id']]
            row=db.execute('SELECT content,COALESCE(search_projection,content),ordinal,conversation_id FROM messages WHERE id=?',(p['original_message'],)).fetchone()
            assert row is not None
            original,represented,ordinal,thread=row
            assert thread==report['cohort'][source['thread']]['original_thread']
            assert ordinal==p['ordinal'] and represented==source['represented_text']
            key=(thread,p['original_message']);assert key not in canonical;canonical.add(key)
            assert hashlib.sha256(original.encode()).hexdigest()==p['original_text_sha256']
            assert hashlib.sha256(represented.encode()).hexdigest()==p['represented_text_sha256']
            assert records[p['id']]['version']==snapshot['versions'][p['id']]
            stored=db.execute('SELECT vector,dimensions,generation FROM message_embeddings WHERE message_id=?',(p['original_message'],)).fetchone()
            expected=records[p['id']]['vector']
            if stored is None:assert expected is None and p['vector_sha256'] is None
            else:
                raw,dimensions,generation=stored
                assert dimensions==384 and len(raw)==384 and generation==report['embedding_generation']
                assert hashlib.sha256(raw).hexdigest()==p['vector_sha256']
                values=struct.unpack('384b',raw);norm=math.sqrt(sum(v*v for v in values))
                assert norm>0 and len(expected)==384
                assert all(abs(value/norm-component)<1e-12 for value,component in zip(values,expected))
                vector_checks+=1
        if 'excluded_source_provenance' in report:
            all_ids=set(by_source);excluded=report['excluded_source_provenance']
            assert len(excluded)==7038
            for entry in excluded:
                assert entry['id'] not in all_ids;all_ids.add(entry['id'])
                original,represented,ordinal,thread,role=db.execute(
                    'SELECT content,COALESCE(search_projection,content),ordinal,conversation_id,role FROM messages WHERE id=?',
                    (entry['original_message'],)).fetchone()
                assert role=='user' and represented=='' and ordinal==entry['ordinal']
                assert thread==report['cohort'][entry['thread']]['original_thread']
                key=(thread,entry['original_message']);assert key not in canonical;canonical.add(key)
                assert hashlib.sha256(original.encode()).hexdigest()==entry['original_text_sha256']
                assert hashlib.sha256(represented.encode()).hexdigest()==entry['represented_text_sha256']
                assert db.execute('SELECT 1 FROM message_embeddings WHERE message_id=?',(entry['original_message'],)).fetchone() is None
            expected_count=sum(db.execute("SELECT count(*) FROM messages WHERE conversation_id=? AND role='user'",(row['original_thread'],)).fetchone()[0]
                               for row in report['cohort'].values())
            assert expected_count==len(all_ids)==8888
            expected_keys={(row['original_thread'],identity) for row in report['cohort'].values()
                           for identity, in db.execute("SELECT id FROM messages WHERE conversation_id=? AND role='user'",(row['original_thread'],))}
            assert canonical==expected_keys
    nodes={n['id']:n for n in snapshot['nodes']}
    assert len(nodes)==len(snapshot['nodes'])
    root=snapshot['root'];seen=set()
    def walk(identity,parent):
        assert identity not in seen
        seen.add(identity);node=nodes[identity]
        assert node['parent']==parent
        if node['children']:
            assert len(node['children'])==2
            children=[walk(child,identity) for child in node['children']]
            expected=set.union(*children)
        else:
            expected={sid for sid,tags in snapshot['memberships'].items() if any(tag['topic']==identity for tag in tags)}
        assert set(node['sources'])==expected
        assert len(node['sources'])==len(expected)
        for label in node['labels']:
            assert label['source_support']>=2
        return expected
    if root:walk(root,None)
    assert seen==set(nodes)
    for row in snapshot['threads']:
        ids={s['id'] for s in sources if s['thread']==row['thread']}
        direct={tag['topic'] for sid in ids for tag in snapshot['memberships'][sid]}
        ancestors=set()
        for leaf in direct:
            parent=nodes[leaf]['parent']
            while parent is not None:
                ancestors.add(parent);parent=nodes[parent]['parent']
        assert set(row['direct_tags'])==direct and set(row['ancestor_tags'])==ancestors
    return {'scope':'source_text_hashes_stored_directions_versions_exclusion_conservation_tree_and_thread_ancestor_unions',
            'sources_checked':len(sources),'nodes_checked':len(nodes),'threads_checked':len(snapshot['threads']),
            'stored_vectors_checked':vector_checks,'excluded_records_checked':len(report.get('excluded_source_provenance',[])),
            'semantic_quality_verified':False,'numerical_routing_reconstructed':False}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--directory',type=Path,required=True)
    parser.add_argument('--database',type=Path,required=True)
    args=parser.parse_args()
    print(json.dumps(audit(args.directory,args.database),sort_keys=True))
