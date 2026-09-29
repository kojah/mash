use super::protocol::{
    Checkpoint, Document, Event, Forget, Forgotten, Indexed, NodeState, Search, Start, Tombstone,
    valid_vector,
};
use super::server::Identity;
use super::{Freshness, NodeFreshness};
use crate::storage::{Counts, Message, SearchHit};
use crate::{AppError, semantic};
use deadpool_postgres::{Pool, Transaction};
use std::collections::HashMap;
use tokio::io::AsyncBufReadExt;
use tokio_postgres::Row;
pub(super) fn conflict(message: &str) -> AppError {
    let mut error = AppError::shared(message);
    error.error.kind = "conflict";
    error
}
pub(super) fn valid_id(id: &str) -> Result<(), AppError> {
    if id.is_empty() || id.len() > 4096 {
        Err(AppError::usage("invalid record identity"))
    } else {
        Ok(())
    }
}
async fn lock_node(
    tx: &Transaction<'_>,
    node: &str,
    start: &Start,
) -> Result<(i64, bool), AppError> {
    valid_id(&start.instance)?;
    if start.revision.is_some_and(|r| r < 1) || start.base_revision.is_some_and(|r| r < 0) {
        return Err(AppError::usage("invalid node revision"));
    }
    tx.execute(
        "INSERT INTO nodes VALUES($1,$2,0) ON CONFLICT DO NOTHING",
        &[&node, &start.instance],
    )
    .await
    .map_err(pg_error)?;
    let row = tx
        .query_one(
            "SELECT instance,revision FROM nodes WHERE node_id=$1 FOR UPDATE",
            &[&node],
        )
        .await
        .map_err(pg_error)?;
    let instance: String = row.try_get(0).map_err(pg_error)?;
    let accepted: i64 = row.try_get(1).map_err(pg_error)?;
    if instance != start.instance {
        return Err(conflict(
            "node_id belongs to another database or indexing mode; choose a new node_id",
        ));
    }
    if start.snapshot {
        let revision = start
            .revision
            .ok_or_else(|| AppError::usage("snapshot requires revision"))?;
        if revision < accepted {
            return Err(conflict("shared index has a newer snapshot"));
        }
        Ok((revision, revision == accepted))
    } else {
        if start.base_revision != Some(accepted) || start.revision.is_some_and(|r| r <= accepted) {
            return Err(conflict(
                "shared index changed while preparing upload; retry indexing",
            ));
        }
        Ok((
            start.revision.unwrap_or(
                accepted
                    .checked_add(1)
                    .ok_or_else(|| conflict("node revision exhausted"))?,
            ),
            false,
        ))
    }
}
async fn forget_in_transaction(
    tx: &Transaction<'_>,
    node: &str,
    id: &str,
) -> Result<bool, AppError> {
    valid_id(id)?;
    let deleted = tx
        .query_opt(
            "DELETE FROM conversations WHERE node_id=$1 AND id=$2 RETURNING provider",
            &[&node, &id],
        )
        .await
        .map_err(pg_error)?;
    if let Some(row) = &deleted {
        let provider: String = row.try_get(0).map_err(pg_error)?;
        tx.execute(
            "INSERT INTO tombstones VALUES($1,$2,$3) ON CONFLICT DO NOTHING",
            &[&node, &provider, &id],
        )
        .await
        .map_err(pg_error)?;
    } else {
        tx.execute("INSERT INTO tombstones SELECT $1,provider,$2 FROM (VALUES('claude-code'),('codex')) p(provider) ON CONFLICT DO NOTHING", &[&node,&id]).await.map_err(pg_error)?;
    }
    Ok(deleted.is_some())
}
pub(super) async fn forget(
    pool: &Pool,
    identity: &Identity,
    input: Forget,
) -> Result<Forgotten, AppError> {
    if input.ids.len() > 10000 {
        return Err(AppError::usage("too many forget identities"));
    }
    let mut connection = connection(pool).await?;
    let mut tx = transaction(&mut connection, identity).await?;
    // Locks protect forget across node processes, while revisions reject restored databases.
    let node = &identity.node_id;
    tx.execute(
        "INSERT INTO nodes VALUES($1,$2,0) ON CONFLICT DO NOTHING",
        &[&node, &input.instance],
    )
    .await
    .map_err(pg_error)?;
    let row = tx
        .query_one(
            "SELECT instance,revision FROM nodes WHERE node_id=$1 FOR UPDATE",
            &[&node],
        )
        .await
        .map_err(pg_error)?;
    let instance: String = row.try_get(0).map_err(pg_error)?;
    let accepted: i64 = row.try_get(1).map_err(pg_error)?;
    if instance != input.instance || input.revision.is_some_and(|r| r <= accepted) {
        return Err(conflict("node identity or revision conflict"));
    }
    let mut forgotten = false;
    for id in &input.ids {
        forgotten |= forget_in_transaction(&tx, node, id).await?;
    }
    let revision = input.revision.unwrap_or(
        accepted
            .checked_add(1)
            .ok_or_else(|| conflict("node revision exhausted"))?,
    );
    tx.execute(
        "UPDATE nodes SET revision=$2 WHERE node_id=$1",
        &[&node, &revision],
    )
    .await
    .map_err(pg_error)?;
    record_sync(&mut tx, node, forgotten, false).await?;
    tx.commit().await.map_err(pg_error)?;
    Ok(Forgotten { forgotten })
}
async fn remove(tx: &Transaction<'_>, node: &str, id: &str) -> Result<(u64, u64), AppError> {
    valid_id(id)?;
    let messages: i64 = tx
        .query_one(
            "SELECT count(*) FROM messages WHERE node_id=$1 AND conversation_id=$2",
            &[&node, &id],
        )
        .await
        .map_err(pg_error)?
        .try_get(0)
        .map_err(pg_error)?;
    let conversations = tx
        .execute(
            "DELETE FROM conversations WHERE node_id=$1 AND id=$2",
            &[&node, &id],
        )
        .await
        .map_err(pg_error)?;
    Ok((messages.cast_unsigned(), conversations))
}
#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ConversationRecord {
    pub id: String,
    pub provider: String,
    pub title: Option<String>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
}
#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MessageRecord {
    pub id: String,
    pub conversation_id: String,
    pub ordinal: i64,
    pub role: String,
    pub content: String,
    pub projection: String,
    pub created_at: Option<i64>,
    pub generation: Option<String>,
    pub vector: Option<Vec<f64>>,
}
async fn upload_batch(
    tx: &Transaction<'_>,
    node: &str,
    kind: &str,
    records: &[serde_json::Value],
    snapshot: bool,
) -> Result<bool, AppError> {
    if records.len() > 128 {
        return Err(AppError::usage("index batch exceeds 128 records"));
    }
    // Validate first, then write one bounded batch per round trip.
    let sql = match kind {
        "tombstones" => {
            for record in records {
                let r: Tombstone = serde_json::from_value(record.clone())
                    .map_err(|_| AppError::usage("invalid tombstone"))?;
                valid_provider(Some(&r.provider))?;
                valid_id(&r.conversation_id)?;
            }
            "INSERT INTO tombstones SELECT $1,provider,conversation_id FROM jsonb_to_recordset($2::text::jsonb) r(provider text,conversation_id text) ON CONFLICT DO NOTHING"
        }
        "conversations" => {
            let mut ids = Vec::new();
            for record in records {
                let r: ConversationRecord = serde_json::from_value(record.clone())
                    .map_err(|_| AppError::usage("invalid conversation"))?;
                valid_provider(Some(&r.provider))?;
                valid_id(&r.id)?;
                ids.push(r.id);
            }
            if !snapshot {
                tx.execute(
                    "DELETE FROM conversations WHERE node_id=$1 AND id=ANY($2)",
                    &[&node, &ids],
                )
                .await
                .map_err(pg_error)?;
            }
            "INSERT INTO conversations SELECT $1,r.id,r.provider,title,created_at,updated_at FROM jsonb_to_recordset($2::text::jsonb) r(id text,provider text,title text,created_at bigint,updated_at bigint) WHERE NOT EXISTS(SELECT 1 FROM tombstones t WHERE t.node_id=$1 AND t.provider=r.provider AND t.conversation_id=r.id)"
        }
        "messages" => {
            for record in records {
                let r: MessageRecord = serde_json::from_value(record.clone())
                    .map_err(|_| AppError::usage("invalid message"))?;
                valid_id(&r.id)?;
                valid_id(&r.conversation_id)?;
                if r.ordinal < 0
                    || r.vector.as_ref().is_some_and(|v| !valid_vector(v))
                    || (r.projection.is_empty() && (r.vector.is_some() || r.generation.is_some()))
                {
                    return Err(AppError::usage("invalid message embedding or ordinal"));
                }
            }
            "INSERT INTO messages(node_id,id,conversation_id,ordinal,role,content,projection,created_at,generation,vector) SELECT $1,r.id,r.conversation_id,ordinal,role,content,projection,created_at,generation,vector::public.vector(384) FROM jsonb_to_recordset($2::text::jsonb) r(id text,conversation_id text,ordinal bigint,role text,content text,projection text,created_at bigint,generation text,vector real[]) WHERE EXISTS(SELECT 1 FROM conversations c WHERE c.node_id=$1 AND c.id=r.conversation_id)"
        }
        _ => return Err(AppError::usage("unknown index batch kind")),
    };
    let json =
        serde_json::to_string(records).map_err(|_| AppError::usage("invalid index batch"))?;
    let affected = tx.execute(sql, &[&node, &json]).await.map_err(pg_error)?;
    if kind == "tombstones" {
        tx.execute("DELETE FROM conversations c USING jsonb_to_recordset($2::text::jsonb) r(provider text,conversation_id text) WHERE c.node_id=$1 AND c.provider=r.provider AND c.id=r.conversation_id", &[&node,&json]).await.map_err(pg_error)?;
    }
    Ok(affected > 0)
}
#[allow(
    clippy::too_many_lines,
    reason = "stream protocol validation and atomic commit share one transaction"
)]
pub(super) async fn index(
    pool: &Pool,
    identity: &Identity,
    file: tempfile::NamedTempFile,
) -> Result<Indexed, AppError> {
    let input = tokio::fs::File::from_std(
        file.reopen()
            .map_err(|_| AppError::internal("open incoming index"))?,
    );
    let mut lines = tokio::io::BufReader::new(input).lines();
    let first = lines
        .next_line()
        .await
        .map_err(|_| AppError::usage("invalid index stream"))?
        .ok_or_else(|| AppError::usage("empty index stream"))?;
    let Event::Start(start) =
        serde_json::from_str(&first).map_err(|_| AppError::usage("invalid index start"))?
    else {
        return Err(AppError::usage("index stream must start with metadata"));
    };
    if start.forgets.len() > 10000
        || (start.snapshot && (start.base_revision.is_some() || !start.forgets.is_empty()))
    {
        return Err(AppError::usage("invalid index metadata"));
    }
    let mut connection = connection(pool).await?;
    let mut tx = transaction(&mut connection, identity).await?;
    let node = &identity.node_id;
    let (revision, duplicate) = lock_node(&tx, node, &start).await?;
    let mut changed = false;
    let mut removed_messages = 0;
    let mut purged_conversations = 0;
    let mut seen: HashMap<String, Vec<String>> = HashMap::new();
    let mut reconciled = std::collections::HashSet::new();
    if !duplicate {
        for id in &start.forgets {
            changed |= forget_in_transaction(&tx, node, id).await?;
        }
        if start.snapshot {
            tx.execute("DELETE FROM direct_sources WHERE node_id=$1", &[&node])
                .await
                .map_err(pg_error)?;
            tx.execute("DELETE FROM conversations WHERE node_id=$1", &[&node])
                .await
                .map_err(pg_error)?;
            changed = true;
        }
    }
    let mut finished = false;
    while let Some(line) = lines
        .next_line()
        .await
        .map_err(|_| AppError::usage("invalid index stream"))?
    {
        if finished {
            return Err(AppError::usage("data after index finish"));
        }
        let event: Event =
            serde_json::from_str(&line).map_err(|_| AppError::usage("invalid index event"))?;
        if matches!(event, Event::Finish) {
            finished = true;
            continue;
        }
        if matches!(event, Event::Start(_)) {
            return Err(AppError::usage("duplicate index start"));
        }
        if duplicate {
            continue;
        }
        match event {
            Event::Batch { kind, records } => {
                changed |= upload_batch(&tx, node, &kind, &records, start.snapshot).await?;
            }
            Event::Source(s) if !start.snapshot => {
                valid_provider(Some(&s.provider))?;
                valid_id(&s.path_hash)?;
                if s.size_bytes < 0 || reconciled.contains(&s.provider) {
                    return Err(AppError::usage("invalid source checkpoint"));
                }
                tx.execute("INSERT INTO direct_sources VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(node_id,provider,path_hash) DO UPDATE SET size_bytes=$4,modified_ns=$5,conversation_id=$6", &[&node,&s.provider,&s.path_hash,&s.size_bytes,&s.modified_ns,&s.conversation_id]).await.map_err(pg_error)?;
            }
            Event::Seen { provider, hashes } if !start.snapshot => {
                valid_provider(Some(&provider))?;
                if hashes.len() > 128 || reconciled.contains(&provider) {
                    return Err(AppError::usage("invalid observed sources"));
                }
                let observed = seen.entry(provider).or_default();
                if observed.len() + hashes.len() > 2_000_000 {
                    return Err(AppError::usage("too many sources"));
                }
                observed.extend(hashes);
            }
            Event::Reconcile { provider, cutoff } if !start.snapshot => {
                valid_provider(Some(&provider))?;
                if !reconciled.insert(provider.clone()) {
                    return Err(AppError::usage("duplicate reconciliation"));
                }
                let observed = seen.get(&provider).cloned().unwrap_or_default();
                let missing=tx.query("DELETE FROM direct_sources WHERE node_id=$1 AND provider=$2 AND NOT(path_hash=ANY($3)) AND ($4::bigint IS NULL OR modified_ns>=$4) RETURNING conversation_id", &[&node,&provider,&observed,&cutoff]).await.map_err(pg_error)?;
                for r in missing {
                    if let Some(id) = r.try_get::<_, Option<String>>(0).map_err(pg_error)? {
                        let referenced=tx.query_opt("SELECT 1 FROM direct_sources WHERE node_id=$1 AND conversation_id=$2", &[&node,&id]).await.map_err(pg_error)?.is_some();
                        if !referenced {
                            let (m, c) = remove(&tx, node, &id).await?;
                            removed_messages += m;
                            purged_conversations += c;
                            changed |= c > 0;
                        }
                    }
                }
            }
            Event::Remove { id } if !start.snapshot => {
                let (m, c) = remove(&tx, node, &id).await?;
                removed_messages += m;
                purged_conversations += c;
                changed |= c > 0;
            }
            _ => {
                return Err(AppError::usage(
                    "invalid index event for this indexing mode",
                ));
            }
        }
    }
    if !finished {
        return Err(AppError::usage("incomplete index stream"));
    }
    if !duplicate {
        tx.execute(
            "UPDATE nodes SET revision=$2 WHERE node_id=$1",
            &[&node, &revision],
        )
        .await
        .map_err(pg_error)?;
    }
    let counts = counts(&mut tx, node).await?;
    record_sync(&mut tx, node, changed, true).await?;
    tx.commit().await.map_err(pg_error)?;
    Ok(Indexed {
        counts,
        revision,
        removed_messages,
        purged_conversations,
    })
}
fn pg_error(error: tokio_postgres::Error) -> AppError {
    let code = error
        .code()
        .map_or("connection", tokio_postgres::error::SqlState::code);
    let result = if code == "40001" || code == "40P01" {
        conflict("concurrent shared update; retry the operation")
    } else if code.starts_with("22") || code.starts_with("23") {
        AppError::usage(format!("invalid shared index data ({code})"))
    } else {
        AppError::shared(format!("PostgreSQL operation failed ({code})"))
    };
    drop(error);
    result
}
pub(super) fn group_schema(group: &str) -> String {
    format!("mash_g_{}", &blake3::hash(group.as_bytes()).to_hex()[..32])
}
async fn transaction<'a>(
    client: &'a mut deadpool_postgres::Object,
    identity: &Identity,
) -> Result<Transaction<'a>, AppError> {
    let tx = client.transaction().await.map_err(pg_error)?;
    tx.batch_execute("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .await
        .map_err(pg_error)?;
    tx.query_one(
        "SELECT set_config('search_path',$1,true)",
        &[&format!("{},public", group_schema(&identity.group_id))],
    )
    .await
    .map_err(pg_error)?;
    tx.batch_execute("SET LOCAL statement_timeout='30s'; SET LOCAL lock_timeout='5s'; SET LOCAL idle_in_transaction_session_timeout='60s'").await.map_err(pg_error)?;
    Ok(tx)
}
async fn connection(pool: &Pool) -> Result<deadpool_postgres::Object, AppError> {
    pool.get()
        .await
        .map_err(|_| AppError::shared("shared database pool is unavailable"))
}
pub(super) async fn initialize(pool: &Pool, groups: &[String]) -> Result<(), AppError> {
    let mut client = connection(pool).await?;
    let tx = client.transaction().await.map_err(pg_error)?;
    tx.batch_execute("SET LOCAL statement_timeout='30s'; SET LOCAL lock_timeout='5s'")
        .await
        .map_err(pg_error)?;
    tx.execute("SELECT pg_advisory_xact_lock(1730294612)", &[])
        .await
        .map_err(pg_error)?;
    tx.batch_execute("CREATE EXTENSION IF NOT EXISTS vector WITH SCHEMA public")
        .await
        .map_err(pg_error)?;
    for group in groups {
        let schema = group_schema(group);
        tx.batch_execute(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema}; SET LOCAL search_path={schema},public;"
        ))
        .await
        .map_err(pg_error)?;
        tx.batch_execute(include_str!("schema.sql"))
            .await
            .map_err(pg_error)?;
        let version: i32 = tx
            .query_one("SELECT version FROM version WHERE singleton", &[])
            .await
            .map_err(pg_error)?
            .try_get(0)
            .map_err(pg_error)?;
        if version != 2 {
            return Err(AppError::shared("unsupported shared index schema version"));
        }
    }
    tx.commit().await.map_err(pg_error)
}
pub(super) async fn freshness(pool: &Pool, config: &Identity) -> Result<Freshness, AppError> {
    let mut connection = connection(pool).await?;
    let client = transaction(&mut connection, config).await?;
    let rows = client
        .query(
            "SELECT n.node_id,n.revision,
         to_char(s.last_synced_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'),
         to_char(s.last_updated_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'),
         (SELECT count(*) FROM conversations c WHERE c.node_id=n.node_id),
         (SELECT count(*) FROM messages m WHERE m.node_id=n.node_id)
         FROM nodes n LEFT JOIN node_sync s USING(node_id) ORDER BY n.node_id",
            &[],
        )
        .await
        .map_err(pg_error)?;
    let nodes = rows
        .into_iter()
        .map(|row| {
            Ok(NodeFreshness {
                node_id: row.try_get(0).map_err(pg_error)?,
                revision: row.try_get(1).map_err(pg_error)?,
                last_synced_at: row.try_get(2).map_err(pg_error)?,
                last_updated_at: row.try_get(3).map_err(pg_error)?,
                conversations: row.try_get(4).map_err(pg_error)?,
                messages: row.try_get(5).map_err(pg_error)?,
            })
        })
        .collect::<Result<_, AppError>>()?;
    Ok(Freshness {
        group_id: config.group_id.clone(),
        nodes,
    })
}

async fn record_sync(
    client: &mut Transaction<'_>,
    node: &str,
    changed: bool,
    checked: bool,
) -> Result<(), AppError> {
    client
        .execute(
            "INSERT INTO node_sync(node_id,last_synced_at,last_updated_at)
         VALUES ($1,CASE WHEN $3 THEN clock_timestamp() END,CASE WHEN $2 THEN clock_timestamp() END)
         ON CONFLICT(node_id) DO UPDATE SET
         last_synced_at=CASE WHEN $3 THEN clock_timestamp() ELSE node_sync.last_synced_at END,
         last_updated_at=CASE WHEN $2 THEN clock_timestamp() ELSE node_sync.last_updated_at END",
            &[&node, &changed, &checked],
        )
        .await
        .map_err(pg_error)?;
    Ok(())
}

pub(super) async fn view(
    pool: &Pool,
    config: &Identity,
    node: &str,
    id: &str,
    context: u32,
) -> Result<Vec<Message>, AppError> {
    let mut connection = connection(pool).await?;
    let client = transaction(&mut connection, config).await?;
    client.query("SELECT m.id,m.ordinal,m.role,m.content,m.created_at FROM messages m
         JOIN messages anchor ON anchor.node_id = m.node_id AND anchor.conversation_id = m.conversation_id
         WHERE anchor.node_id = $1 AND anchor.id = $2
           AND m.ordinal BETWEEN anchor.ordinal - $3 AND anchor.ordinal + $3 ORDER BY m.ordinal", &[&node,&id,&i64::from(context)]).await.map_err(pg_error)?.iter().map(|row| Ok(Message {
        id: row.try_get(0).map_err(pg_error)?, ordinal: row.try_get(1).map_err(pg_error)?,
        role: row.try_get(2).map_err(pg_error)?, content: row.try_get(3).map_err(pg_error)?,
        created_at: row.try_get(4).map_err(pg_error)?,
    })).collect()
}

fn hit(row: &Row, lexical: bool) -> Result<SearchHit, AppError> {
    let score: f64 = row.try_get(7).map_err(pg_error)?;
    #[allow(clippy::cast_possible_truncation)]
    let semantic_score = (!lexical).then_some(score as f32);
    Ok(SearchHit {
        node_id: Some(row.try_get(0).map_err(pg_error)?),
        id: row.try_get(1).map_err(pg_error)?,
        conversation_id: row.try_get(2).map_err(pg_error)?,
        provider: row.try_get(3).map_err(pg_error)?,
        role: row.try_get(4).map_err(pg_error)?,
        content: row.try_get(5).map_err(pg_error)?,
        lexical_score: lexical.then_some(-score),
        semantic_score,
        fusion_score: 0.0,
        rerank_score: None,
    })
}

struct Candidates {
    lexical: Vec<SearchHit>,
    semantic: Vec<SearchHit>,
    documents: HashMap<(String, String), String>,
}

async fn candidates(
    client: &mut Transaction<'_>,
    query: &str,
    vector: &[f64],
    provider: Option<&str>,
    days: Option<u32>,
) -> Result<Candidates, AppError> {
    let cutoff = days.map(|days| {
        i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        )
        .unwrap_or(i64::MAX)
        .saturating_sub(i64::from(days) * 86400)
    });
    let generation = semantic::SHARED_EMBEDDING_GENERATION;
    let vector =
        serde_json::to_string(vector).map_err(|_| AppError::usage("invalid query vector"))?;
    let lexical = client
        .query(
            "SELECT m.node_id,m.id,m.conversation_id,c.provider,m.role,m.content,m.projection,
                ts_rank_cd(document, websearch_to_tsquery('simple', $1))::double precision AS score
         FROM messages m JOIN conversations c ON c.node_id=m.node_id AND c.id=m.conversation_id
         WHERE document @@ websearch_to_tsquery('simple', $1) AND projection <> '' AND generation=$2
           AND ($3::text IS NULL OR c.provider=$3) AND ($4::bigint IS NULL OR m.created_at >= $4)
         ORDER BY score DESC,m.node_id,m.id LIMIT 50",
            &[&query, &generation, &provider, &cutoff],
        )
        .await
        .map_err(pg_error)?;
    let semantic = client
        .query(
            "SELECT m.node_id,m.id,m.conversation_id,c.provider,m.role,m.content,m.projection,
                1.0 - (m.vector <=> $1::text::public.vector) AS score
         FROM messages m JOIN conversations c ON c.node_id=m.node_id AND c.id=m.conversation_id
         WHERE generation=$2 AND vector_dims(vector)=384 AND projection <> ''
           AND ($3::text IS NULL OR c.provider=$3) AND ($4::bigint IS NULL OR m.created_at >= $4)
         ORDER BY score DESC,m.node_id,m.id LIMIT 50",
            &[&vector, &generation, &provider, &cutoff],
        )
        .await
        .map_err(pg_error)?;
    let mut documents = HashMap::new();
    for row in lexical.iter().chain(&semantic) {
        documents.insert(
            (
                row.try_get(0).map_err(pg_error)?,
                row.try_get(1).map_err(pg_error)?,
            ),
            row.try_get(6).map_err(pg_error)?,
        );
    }
    Ok(Candidates {
        lexical: lexical
            .iter()
            .map(|r| hit(r, true))
            .collect::<Result<_, _>>()?,
        semantic: semantic
            .iter()
            .map(|r| hit(r, false))
            .collect::<Result<_, _>>()?,
        documents,
    })
}

pub(super) async fn search(
    pool: &Pool,
    identity: &Identity,
    input: Search,
) -> Result<super::protocol::Candidates, AppError> {
    if input.query.trim().is_empty() || input.query.len() > 16384 || !valid_vector(&input.vector) {
        return Err(AppError::usage("invalid search query or vector"));
    }
    valid_provider(input.provider.as_deref())?;
    let mut connection = connection(pool).await?;
    let mut tx = transaction(&mut connection, identity).await?;
    let incompatible: bool = tx.query_one("SELECT EXISTS(SELECT 1 FROM messages WHERE projection <> '' AND (generation IS DISTINCT FROM $1 OR vector IS NULL OR vector_dims(vector)<>384))", &[&semantic::SHARED_EMBEDDING_GENERATION]).await.map_err(pg_error)?.try_get(0).map_err(pg_error)?;
    if incompatible {
        return Err(AppError::shared(
            "shared embeddings are incomplete or incompatible; run mash index on publishing nodes",
        ));
    }
    let found = candidates(
        &mut tx,
        &input.query,
        &input.vector,
        input.provider.as_deref(),
        input.days,
    )
    .await?;
    tx.commit().await.map_err(pg_error)?;
    Ok(super::protocol::Candidates {
        lexical: found.lexical,
        semantic: found.semantic,
        documents: found
            .documents
            .into_iter()
            .map(|((node_id, id), projection)| Document {
                node_id,
                id,
                projection,
            })
            .collect(),
    })
}
pub(super) fn valid_provider(provider: Option<&str>) -> Result<(), AppError> {
    if provider.is_some_and(|p| !matches!(p, "codex" | "claude-code")) {
        Err(AppError::usage("unknown provider"))
    } else {
        Ok(())
    }
}
pub(super) async fn counts(tx: &mut Transaction<'_>, node: &str) -> Result<Counts, AppError> {
    let row = tx.query_one("SELECT (SELECT count(*) FROM conversations WHERE node_id=$1), count(*), count(*) FILTER(WHERE projection<>''), count(*) FILTER(WHERE projection<>'' AND generation=$2 AND vector_dims(vector)=384) FROM messages WHERE node_id=$1", &[&node,&semantic::SHARED_EMBEDDING_GENERATION]).await.map_err(pg_error)?;
    Ok(Counts {
        conversations: row.try_get::<_, i64>(0).map_err(pg_error)?.cast_unsigned(),
        messages: row.try_get::<_, i64>(1).map_err(pg_error)?.cast_unsigned(),
        searchable_messages: row.try_get::<_, i64>(2).map_err(pg_error)?.cast_unsigned(),
        embeddings: row.try_get::<_, i64>(3).map_err(pg_error)?.cast_unsigned(),
    })
}
pub(super) async fn status(pool: &Pool, identity: &Identity) -> Result<Counts, AppError> {
    let mut connection = connection(pool).await?;
    let mut tx = transaction(&mut connection, identity).await?;
    counts(&mut tx, &identity.node_id).await
}
pub(super) async fn state(pool: &Pool, identity: &Identity) -> Result<NodeState, AppError> {
    let mut connection = connection(pool).await?;
    let tx = transaction(&mut connection, identity).await?;
    let node = tx
        .query_opt(
            "SELECT instance,revision FROM nodes WHERE node_id=$1",
            &[&&identity.node_id],
        )
        .await
        .map_err(pg_error)?;
    let (instance, revision) = if let Some(row) = node {
        (
            Some(row.try_get(0).map_err(pg_error)?),
            row.try_get(1).map_err(pg_error)?,
        )
    } else {
        (None, 0)
    };
    let mut sources = Vec::new();
    for r in tx.query("SELECT provider,path_hash,size_bytes,modified_ns,conversation_id, NOT EXISTS(SELECT 1 FROM messages m WHERE m.node_id=s.node_id AND m.conversation_id=s.conversation_id AND projection<>'' AND (generation IS DISTINCT FROM $2 OR vector IS NULL OR vector_dims(vector)<>384)) FROM direct_sources s WHERE node_id=$1", &[&&identity.node_id,&semantic::SHARED_EMBEDDING_GENERATION]).await.map_err(pg_error)? {
        sources.push(Checkpoint {provider:r.try_get(0).map_err(pg_error)?, path_hash:r.try_get(1).map_err(pg_error)?,size_bytes:r.try_get(2).map_err(pg_error)?,modified_ns:r.try_get(3).map_err(pg_error)?,conversation_id:r.try_get(4).map_err(pg_error)?,ready:r.try_get(5).map_err(pg_error)?});
    }
    let mut tombstones = Vec::new();
    for r in tx
        .query(
            "SELECT provider,conversation_id FROM tombstones WHERE node_id=$1",
            &[&&identity.node_id],
        )
        .await
        .map_err(pg_error)?
    {
        tombstones.push(Tombstone {
            provider: r.try_get(0).map_err(pg_error)?,
            conversation_id: r.try_get(1).map_err(pg_error)?,
        });
    }
    tx.commit().await.map_err(pg_error)?;
    Ok(NodeState {
        instance,
        revision,
        sources,
        tombstones,
    })
}
