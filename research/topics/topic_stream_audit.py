"""Independent artifact/source and structural checks; not semantic-quality validation."""
import argparse
import hashlib
import json
from pathlib import Path
import sqlite3


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
    with sqlite3.connect(database.resolve().as_uri()+'?mode=ro&immutable=1',uri=True) as db:
        for p in report['source_provenance']:
            source=by_source[p['id']]
            row=db.execute('SELECT content,COALESCE(search_projection,content),ordinal,conversation_id FROM messages WHERE id=?',(p['original_message'],)).fetchone()
            assert row is not None
            original,represented,ordinal,thread=row
            assert thread==report['cohort'][source['thread']]['original_thread']
            assert ordinal==p['ordinal'] and represented==source['represented_text']
            assert hashlib.sha256(original.encode()).hexdigest()==p['original_text_sha256']
            assert hashlib.sha256(represented.encode()).hexdigest()==p['represented_text_sha256']
            assert records[p['id']]['version']==snapshot['versions'][p['id']]
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
    return {'scope':'source_text_hashes_versions_tree_source_unions_and_thread_ancestor_unions',
            'sources_checked':len(sources),'nodes_checked':len(nodes),'threads_checked':len(snapshot['threads']),
            'semantic_quality_verified':False,'numerical_routing_reconstructed':False}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--directory',type=Path,required=True)
    parser.add_argument('--database',type=Path,required=True)
    args=parser.parse_args()
    print(json.dumps(audit(args.directory,args.database),sort_keys=True))
