--! node
SELECT instance, revision FROM nodes WHERE node_id=:node FOR UPDATE;

--! thread_catalog : (title?, project?, repository?, created_at?, updated_at?)
SELECT node_id,id,provider,title,project,repository,created_at,updated_at
FROM conversations;

--! node_state
SELECT instance, revision FROM nodes WHERE node_id=:node;

--! version
SELECT version FROM version WHERE singleton;

--! counts
SELECT (SELECT count(*) FROM conversations WHERE node_id=:node) AS conversations,
 count(*) AS messages, count(*) FILTER(WHERE projection<>'') AS searchable_messages,
 count(*) FILTER(WHERE projection<>'' AND generation=:generation AND vector_dims(vector)=:dimensions) AS embeddings
FROM messages WHERE node_id=:node;

--! incompatible (exclude_node?)
SELECT EXISTS(SELECT 1 FROM messages WHERE projection<>'' AND (:exclude_node::text IS NULL OR node_id<>:exclude_node)
 AND (generation IS DISTINCT FROM :generation OR vector IS NULL OR vector_dims(vector)<>:dimensions)) AS incompatible;

--! view : (created_at?)
WITH RECURSIVE anchor AS (SELECT * FROM messages WHERE node_id=:node AND id=:id),
ancestors(ordinal,depth) AS (
    SELECT ordinal,0::bigint FROM anchor WHERE context_parent IS NOT NULL
    UNION ALL
    SELECT m.context_parent,a.depth+1 FROM messages m JOIN ancestors a ON m.ordinal=a.ordinal
    JOIN anchor ON m.node_id=anchor.node_id AND m.conversation_id=anchor.conversation_id
    WHERE m.context_parent>=0 AND a.depth<:context
),
descendants(ordinal,depth) AS (
    SELECT ordinal,0::bigint FROM anchor WHERE context_parent IS NOT NULL
    UNION ALL
    SELECT (SELECT MIN(m.ordinal) FROM messages m WHERE m.node_id=anchor.node_id AND m.conversation_id=anchor.conversation_id AND m.context_parent=d.ordinal),d.depth+1
    FROM descendants d CROSS JOIN anchor WHERE d.depth<:context AND EXISTS(SELECT 1 FROM messages m WHERE m.node_id=anchor.node_id AND m.conversation_id=anchor.conversation_id AND m.context_parent=d.ordinal)
)
SELECT m.id,m.ordinal,m.role,m.content,m.projection,m.created_at FROM messages m JOIN anchor ON m.node_id=anchor.node_id AND m.conversation_id=anchor.conversation_id
WHERE anchor.context_parent IS NULL AND m.ordinal BETWEEN anchor.ordinal-:context AND anchor.ordinal+:context
UNION ALL
SELECT m.id,m.ordinal,m.role,m.content,m.projection,m.created_at FROM messages m JOIN anchor ON m.node_id=anchor.node_id AND m.conversation_id=anchor.conversation_id
WHERE m.ordinal IN (SELECT ordinal FROM ancestors UNION SELECT ordinal FROM descendants) ORDER BY ordinal;

--! freshness : (last_synced_at?, last_updated_at?)
SELECT n.node_id,n.revision,
to_char(s.last_synced_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"') AS last_synced_at,
to_char(s.last_updated_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"') AS last_updated_at,
(SELECT count(*) FROM conversations c WHERE c.node_id=n.node_id) AS conversations,
(SELECT count(*) FROM messages m WHERE m.node_id=n.node_id) AS messages
FROM nodes n LEFT JOIN node_sync s USING(node_id) ORDER BY n.node_id;

--! sources : (conversation_id?)
SELECT provider,path_hash,size_bytes,modified_ns,conversation_id,fingerprint,
NOT EXISTS(SELECT 1 FROM messages m WHERE m.node_id=s.node_id AND m.conversation_id=s.conversation_id AND projection<>''
AND (generation IS DISTINCT FROM :generation OR vector IS NULL OR vector_dims(vector)<>:dimensions)) AS ready
FROM direct_sources s WHERE node_id=:node;

--! tombstones
SELECT provider,conversation_id FROM tombstones WHERE node_id=:node;

--! broken_context
SELECT EXISTS(SELECT 1 FROM messages m WHERE m.node_id=:node AND m.context_parent>=0
AND NOT EXISTS(SELECT 1 FROM messages p WHERE p.node_id=m.node_id AND p.conversation_id=m.conversation_id AND p.ordinal=m.context_parent)) AS broken;

--! search (provider?, cutoff?, exclude_node?) : (created_at?, title?, project?, repository?, conversation_created_at?, conversation_updated_at?)
SELECT m.node_id,m.id,m.conversation_id,c.provider,m.role,m.projection,
ts_rank_cd(document,websearch_to_tsquery('simple',:query))::double precision AS score,
m.created_at,c.title,c.project,c.repository,c.created_at AS conversation_created_at,c.updated_at AS conversation_updated_at
FROM messages m JOIN conversations c ON c.node_id=m.node_id AND c.id=m.conversation_id
WHERE document @@ websearch_to_tsquery('simple',:query) AND projection<>'' AND generation=:generation
AND (:provider::text IS NULL OR c.provider=:provider) AND (:cutoff::bigint IS NULL OR m.created_at>=:cutoff)
AND (:exclude_node::text IS NULL OR m.node_id<>:exclude_node)
ORDER BY score DESC,m.node_id,m.id LIMIT :limit;

--! semantic (provider?, cutoff?, exclude_node?) : Search(created_at?, title?, project?, repository?, conversation_created_at?, conversation_updated_at?)
SELECT m.node_id,m.id,m.conversation_id,c.provider,m.role,m.projection,
1.0-(m.vector <=> :vector::text::public.vector) AS score,
m.created_at,c.title,c.project,c.repository,c.created_at AS conversation_created_at,c.updated_at AS conversation_updated_at
FROM messages m JOIN conversations c ON c.node_id=m.node_id AND c.id=m.conversation_id
WHERE generation=:generation AND vector_dims(vector)=:dimensions AND projection<>''
AND (:provider::text IS NULL OR c.provider=:provider) AND (:cutoff::bigint IS NULL OR m.created_at>=:cutoff)
AND (:exclude_node::text IS NULL OR m.node_id<>:exclude_node)
ORDER BY score DESC,m.node_id,m.id LIMIT :limit;
