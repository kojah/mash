--! node
SELECT instance, revision FROM nodes WHERE node_id=:node FOR UPDATE;

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
SELECT m.id,m.ordinal,m.role,m.content,m.projection,m.created_at FROM messages m
JOIN messages anchor ON anchor.node_id=m.node_id AND anchor.conversation_id=m.conversation_id
WHERE anchor.node_id=:node AND anchor.id=:id AND m.ordinal BETWEEN anchor.ordinal-:context AND anchor.ordinal+:context ORDER BY m.ordinal;

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
