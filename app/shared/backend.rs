//! PostgreSQL storage adapter: silver history and the gold `message_search` table.
//! Schema/migrations own delineation; publication keeps both in one transaction.
use super::protocol::{
    Batch, Checkpoint, Document, Event, Forget, Forgotten, Indexed, NodeState, Search, Start,
    Tombstone, valid_vector,
};
use super::server::Identity;
use super::{Freshness, NodeFreshness};
use crate::AppError;
use crate::provider::Provider;
use crate::storage::{Counts, Message, SearchHit};
use deadpool_postgres::{Pool, Transaction};
use mash_postgres::queries::index as pg;
use std::collections::HashMap;
use tokio::io::AsyncBufReadExt;
use tokio_postgres::Row;
mod pool_error;
pub(super) fn conflict(message: &str) -> AppError {
    let mut error = AppError::shared(message);
    error.error.kind = crate::ErrorKind::Conflict;
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
    let row = pg::node().bind(tx, &node).one().await.map_err(pg_error)?;
    let instance = row.instance;
    let accepted = row.revision;
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
        let provider: Provider = row.try_get("provider").map_err(pg_error)?;
        tx.execute(
            "INSERT INTO tombstones VALUES($1,$2,$3) ON CONFLICT DO NOTHING",
            &[&node, &provider.as_str(), &id],
        )
        .await
        .map_err(pg_error)?;
    } else {
        let providers = Provider::ALL
            .iter()
            .map(|provider| provider.as_str())
            .collect::<Vec<_>>();
        tx.execute("INSERT INTO tombstones SELECT $1,provider,$2 FROM unnest($3::text[]) p(provider) ON CONFLICT DO NOTHING", &[&node,&id,&providers]).await.map_err(pg_error)?;
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
    let row = pg::node().bind(&tx, &node).one().await.map_err(pg_error)?;
    let instance = row.instance;
    let accepted = row.revision;
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
        .try_get("count")
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
async fn upload_batch(
    tx: &Transaction<'_>,
    node: &str,
    batch: &Batch,
    snapshot: bool,
    dimensions: usize,
) -> Result<bool, AppError> {
    if batch.len() > 128 {
        return Err(AppError::usage("index batch exceeds 128 records"));
    }
    // Validate first, then write one bounded batch per round trip.
    let sql = match batch {
        Batch::Tombstones { records } => {
            for r in records {
                valid_id(&r.conversation_id)?;
            }
            "INSERT INTO tombstones SELECT $1,provider,conversation_id FROM jsonb_to_recordset($2::text::jsonb) r(provider text,conversation_id text) ON CONFLICT DO NOTHING"
        }
        Batch::Conversations { records } => {
            let mut ids = Vec::new();
            for r in records {
                valid_id(&r.id)?;
                ids.push(&r.id);
            }
            if !snapshot {
                tx.execute(
                    "DELETE FROM conversations WHERE node_id=$1 AND id=ANY($2)",
                    &[&node, &ids],
                )
                .await
                .map_err(pg_error)?;
            }
            "INSERT INTO conversations(node_id,id,provider,title,created_at,updated_at,project,repository) SELECT $1,r.id,r.provider,title,created_at,updated_at,project,repository FROM jsonb_to_recordset($2::text::jsonb) r(id text,provider text,title text,created_at bigint,updated_at bigint,project text,repository text) WHERE NOT EXISTS(SELECT 1 FROM tombstones t WHERE t.node_id=$1 AND t.provider=r.provider AND t.conversation_id=r.id)"
        }
        Batch::Messages { records } => {
            for r in records {
                valid_id(&r.id)?;
                valid_id(&r.conversation_id)?;
                if r.ordinal < 0
                    || !r.context.valid(r.ordinal)
                    || r.vector
                        .as_ref()
                        .is_some_and(|v| !valid_vector(v, dimensions))
                    || (r.projection.is_empty() && (r.vector.is_some() || r.generation.is_some()))
                {
                    return Err(AppError::usage("invalid message embedding or ordinal"));
                }
            }
            "WITH silver AS (INSERT INTO messages(node_id,id,conversation_id,ordinal,role,content,projection,created_at,context_parent) SELECT $1,r.id,r.conversation_id,ordinal,role,content,projection,created_at,CASE context->>'kind' WHEN 'root' THEN -1 WHEN 'parent' THEN (context->>'ordinal')::bigint END FROM jsonb_to_recordset($2::text::jsonb) r(id text,conversation_id text,ordinal bigint,role text,content text,projection text,created_at bigint,context jsonb) WHERE EXISTS(SELECT 1 FROM conversations c WHERE c.node_id=$1 AND c.id=r.conversation_id) RETURNING node_id,id,projection) INSERT INTO message_search(node_id,id,generation,vector,document) SELECT s.node_id,s.id,r.generation,r.vector::public.vector,to_tsvector('simple',s.projection) FROM silver s JOIN jsonb_to_recordset($2::text::jsonb) r(id text,generation text,vector real[]) ON r.id=s.id"
        }
    };
    let json = batch
        .records_json()
        .map_err(|_| AppError::usage("invalid index batch"))?;
    let affected = tx.execute(sql, &[&node, &json]).await.map_err(pg_error)?;
    if matches!(batch, Batch::Tombstones { .. }) {
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
    let mut seen: HashMap<Provider, Vec<String>> = HashMap::new();
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
            Event::Batch(batch) => {
                changed |=
                    upload_batch(&tx, node, &batch, start.snapshot, identity.dimensions).await?;
            }
            Event::Source(s) if !start.snapshot => {
                valid_id(&s.path_hash)?;
                if s.size_bytes < 0 || reconciled.contains(&s.provider) {
                    return Err(AppError::usage("invalid source checkpoint"));
                }
                tx.execute("INSERT INTO direct_sources(node_id,provider,path_hash,size_bytes,modified_ns,conversation_id,fingerprint,interpretation_version) VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT(node_id,provider,path_hash) DO UPDATE SET size_bytes=$4,modified_ns=$5,conversation_id=$6,fingerprint=$7,interpretation_version=$8", &[&node,&s.provider.as_str(),&s.path_hash,&s.size_bytes,&s.modified_ns,&s.conversation_id,&s.fingerprint,&i64::from(s.interpretation_version)]).await.map_err(pg_error)?;
            }
            Event::Seen { provider, hashes } if !start.snapshot => {
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
                if !reconciled.insert(provider) {
                    return Err(AppError::usage("duplicate reconciliation"));
                }
                let observed = seen.get(&provider).cloned().unwrap_or_default();
                let missing=tx.query("DELETE FROM direct_sources WHERE node_id=$1 AND provider=$2 AND NOT(path_hash=ANY($3)) AND ($4::bigint IS NULL OR modified_ns>=$4) RETURNING conversation_id", &[&node,&provider.as_str(),&observed,&cutoff]).await.map_err(pg_error)?;
                for r in missing {
                    if let Some(id) = r
                        .try_get::<_, Option<String>>("conversation_id")
                        .map_err(pg_error)?
                    {
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
    if pg::broken_context()
        .bind(&tx, &node)
        .one()
        .await
        .map_err(pg_error)?
    {
        return Err(AppError::usage("missing message context parent"));
    }
    if !duplicate {
        tx.execute(
            "UPDATE nodes SET revision=$2 WHERE node_id=$1",
            &[&node, &revision],
        )
        .await
        .map_err(pg_error)?;
    }
    let counts = counts(&mut tx, node, identity).await?;
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

pub(super) async fn list_threads(
    pool: &Pool,
    identity: &Identity,
    input: crate::thread_catalog::Request,
) -> Result<crate::thread_catalog::Page, AppError> {
    use futures_util::TryStreamExt;
    let mut collector = crate::thread_catalog::Collector::new(input.clone())?;
    let mut connection = connection(pool).await?;
    let client = transaction(&mut connection, identity).await?;
    let statement = pg::thread_catalog();
    let rows = statement.bind(&client).iter().await.map_err(pg_error)?;
    futures_util::pin_mut!(rows);
    while let Some(row) = rows.try_next().await.map_err(pg_error)? {
        if !input.admits_node(&row.node_id, &identity.node_id) {
            continue;
        }
        let provider = row
            .provider
            .parse()
            .map_err(|_| AppError::shared("invalid shared thread provider"))?;
        collector.push(crate::thread_catalog::Thread {
            conversation_id: row.id,
            provider,
            node_id: row.node_id,
            metadata: crate::search_results::ThreadMetadata {
                title: row.title,
                project: row.project,
                repository: row.repository,
                created_at: row.created_at,
                updated_at: row.updated_at,
            },
        });
    }
    Ok(collector.finish())
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
    tx.batch_execute("SET LOCAL standard_conforming_strings=on; SET LOCAL statement_timeout='30s'; SET LOCAL lock_timeout='5s'; SET LOCAL idle_in_transaction_session_timeout='60s'").await.map_err(pg_error)?;
    Ok(tx)
}
async fn connection(pool: &Pool) -> Result<deadpool_postgres::Object, AppError> {
    pool.get().await.map_err(pool_error::diagnostic)
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
        tx.batch_execute(include_str!("migrations.sql"))
            .await
            .map_err(pg_error)?;
        tx.batch_execute(include_str!("schema.sql"))
            .await
            .map_err(pg_error)?;
        let version = pg::version().bind(&tx).one().await.map_err(pg_error)?;
        if version != 17 {
            return Err(AppError::shared("unsupported shared index schema version"));
        }
    }
    tx.commit().await.map_err(pg_error)
}
pub(super) async fn freshness(pool: &Pool, config: &Identity) -> Result<Freshness, AppError> {
    let mut connection = connection(pool).await?;
    let client = transaction(&mut connection, config).await?;
    let rows = pg::freshness()
        .bind(&client)
        .all()
        .await
        .map_err(pg_error)?;
    let nodes = rows
        .into_iter()
        .map(|row| NodeFreshness {
            node_id: row.node_id,
            revision: row.revision,
            last_synced_at: row.last_synced_at,
            last_updated_at: row.last_updated_at,
            conversations: row.conversations,
            messages: row.messages,
        })
        .collect();
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
    representation: crate::storage::MessageContent,
) -> Result<Vec<Message>, AppError> {
    let mut connection = connection(pool).await?;
    let client = transaction(&mut connection, config).await?;
    let rows = pg::view()
        .bind(&client, &node, &id, &i64::from(context))
        .all()
        .await
        .map_err(pg_error)?;
    Ok(rows
        .into_iter()
        .map(|row| Message {
            id: row.id,
            ordinal: row.ordinal,
            role: row.role.into(),
            content: match representation {
                crate::storage::MessageContent::Canonical => row.content,
                crate::storage::MessageContent::SearchProjection => row.projection,
            },
            created_at: row.created_at,
        })
        .collect())
}

fn hit(row: pg::Search, lexical: bool) -> Result<SearchHit, AppError> {
    let score = row.score;
    #[allow(clippy::cast_possible_truncation)]
    let semantic_score = (!lexical).then_some(score as f32);
    Ok(SearchHit {
        node_id: Some(row.node_id),
        id: row.id,
        conversation_id: row.conversation_id,
        provider: row.provider.parse()?,
        role: row.role.into(),
        content: row.projection,
        copies: Vec::new(),
        created_at: row.created_at,
        metadata: crate::search_results::ThreadMetadata {
            title: row.title,
            project: row.project,
            repository: row.repository,
            created_at: row.conversation_created_at,
            updated_at: row.conversation_updated_at,
        },
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

// Expressive filters have a runtime SQL predicate. The projection matches the
// generated search model, with named decoding for this dynamic-query boundary.
fn filtered_search_row(row: &Row) -> Result<pg::Search, AppError> {
    Ok(pg::Search {
        node_id: row.try_get("node_id").map_err(pg_error)?,
        id: row.try_get("id").map_err(pg_error)?,
        conversation_id: row.try_get("conversation_id").map_err(pg_error)?,
        provider: row.try_get("provider").map_err(pg_error)?,
        role: row.try_get("role").map_err(pg_error)?,
        projection: row.try_get("projection").map_err(pg_error)?,
        score: row.try_get("score").map_err(pg_error)?,
        created_at: row.try_get("created_at").map_err(pg_error)?,
        title: row.try_get("title").map_err(pg_error)?,
        project: row.try_get("project").map_err(pg_error)?,
        repository: row.try_get("repository").map_err(pg_error)?,
        conversation_created_at: row.try_get("conversation_created_at").map_err(pg_error)?,
        conversation_updated_at: row.try_get("conversation_updated_at").map_err(pg_error)?,
    })
}

async fn filtered_candidates(
    client: &Transaction<'_>,
    params: pg::SearchParams<&str, &str, &str, &str>,
    vector: &str,
    dimensions: i32,
    predicate: &str,
) -> Result<(Vec<pg::Search>, Vec<pg::Search>), AppError> {
    let pg::SearchParams {
        query,
        generation,
        provider,
        cutoff,
        exclude_node,
        limit: candidate_limit,
    } = params;
    let lexical = client
        .query(
            &format!(
                "SELECT m.node_id,m.id,m.conversation_id,c.provider,m.role,m.content,m.projection,
                ts_rank_cd(document, websearch_to_tsquery('simple', $1))::double precision AS score, m.created_at,c.title,c.project,c.repository,c.created_at AS conversation_created_at,c.updated_at AS conversation_updated_at
         FROM messages m JOIN message_search g ON g.node_id=m.node_id AND g.id=m.id JOIN conversations c ON c.node_id=m.node_id AND c.id=m.conversation_id
         WHERE document @@ websearch_to_tsquery('simple', $1) AND projection <> '' AND generation=$2
           AND ($3::text IS NULL OR c.provider=$3) AND ($4::bigint IS NULL OR m.created_at >= $4)
           AND ($5::text IS NULL OR m.node_id<>$5)
           AND ({predicate})
         ORDER BY score DESC,m.node_id,m.id LIMIT {candidate_limit}"
            ),
            &[&query, &generation, &provider, &cutoff, &exclude_node],
        )
        .await
        .map_err(pg_error)?;
    let semantic = client
        .query(
            &format!(
                "SELECT m.node_id,m.id,m.conversation_id,c.provider,m.role,m.content,m.projection,
                1.0 - (g.vector <=> $1::text::public.vector) AS score, m.created_at,c.title,c.project,c.repository,c.created_at AS conversation_created_at,c.updated_at AS conversation_updated_at
         FROM messages m JOIN message_search g ON g.node_id=m.node_id AND g.id=m.id JOIN conversations c ON c.node_id=m.node_id AND c.id=m.conversation_id
         WHERE generation=$2 AND vector_dims(vector)=$5 AND projection <> ''
           AND ($3::text IS NULL OR c.provider=$3) AND ($4::bigint IS NULL OR m.created_at >= $4)
           AND ($6::text IS NULL OR m.node_id<>$6)
           AND ({predicate})
         ORDER BY score DESC,m.node_id,m.id LIMIT {candidate_limit}"
            ),
            &[
                &vector,
                &generation,
                &provider,
                &cutoff,
                &dimensions,
                &exclude_node,
            ],
        )
        .await
        .map_err(pg_error)?;
    Ok((
        lexical
            .iter()
            .map(filtered_search_row)
            .collect::<Result<Vec<_>, _>>()?,
        semantic
            .iter()
            .map(filtered_search_row)
            .collect::<Result<Vec<_>, _>>()?,
    ))
}

async fn candidates(
    client: &mut Transaction<'_>,
    query: &str,
    vector: &[f64],
    identity: &Identity,
    legacy: (
        Option<crate::provider::Provider>,
        Option<u32>,
        Option<&str>,
        crate::search_results::SearchUnit,
    ),
    filter: Option<&crate::search_filter::Filter>,
) -> Result<Candidates, AppError> {
    let (provider, days, exclude_node, unit) = legacy;
    let candidate_limit = unit.candidate_limit();
    let provider = provider.map(Provider::as_str);
    let predicate = filter.map_or_else(|| "TRUE".to_owned(), |f| f.sql(None));
    let cutoff = days.map(|days| {
        i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        )
        .unwrap_or(i64::MAX)
        .saturating_sub(i64::from(days) * 86400)
        .saturating_mul(1000)
    });
    let generation = &identity.generation;
    let dimensions =
        i32::try_from(identity.dimensions).map_err(|_| AppError::usage("invalid dimensions"))?;
    let vector =
        serde_json::to_string(vector).map_err(|_| AppError::usage("invalid query vector"))?;
    let (lexical, semantic) = if filter.is_none() {
        let limit = i64::try_from(candidate_limit).map_err(|_| AppError::usage("invalid limit"))?;
        let lexical = pg::search()
            .bind(
                client,
                &query,
                generation,
                &provider,
                &cutoff,
                &exclude_node,
                &limit,
            )
            .all()
            .await
            .map_err(pg_error)?;
        let semantic = pg::semantic()
            .bind(
                client,
                &vector,
                generation,
                &dimensions,
                &provider,
                &cutoff,
                &exclude_node,
                &limit,
            )
            .all()
            .await
            .map_err(pg_error)?;
        (lexical, semantic)
    } else {
        filtered_candidates(
            client,
            pg::SearchParams {
                query,
                generation,
                provider,
                cutoff,
                exclude_node,
                limit: i64::try_from(candidate_limit)
                    .map_err(|_| AppError::usage("invalid limit"))?,
            },
            &vector,
            dimensions,
            &predicate,
        )
        .await?
    };
    let documents = lexical
        .iter()
        .chain(&semantic)
        .map(|row| {
            (
                (row.node_id.clone(), row.id.clone()),
                row.projection.clone(),
            )
        })
        .collect();
    Ok(Candidates {
        lexical: lexical
            .into_iter()
            .map(|r| hit(r, true))
            .collect::<Result<_, _>>()?,
        semantic: semantic
            .into_iter()
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
    crate::search_query::validate(&input.query)?;
    if !crate::search_query::has_terms(&input.query)
        || input.query.len() > 16384
        || !valid_vector(&input.vector, identity.dimensions)
        || input.generation != identity.generation
    {
        return Err(AppError::usage("invalid search query or vector"));
    }

    if let Some(filter) = &input.filter {
        filter.validate()?;
    }
    let mut connection = connection(pool).await?;
    let mut tx = transaction(&mut connection, identity).await?;
    let exclude_node = input.exclude_self.then_some(identity.node_id.as_str());
    let incompatible = pg::incompatible()
        .bind(
            &tx,
            &exclude_node,
            &identity.generation,
            &i32::try_from(identity.dimensions)
                .map_err(|_| AppError::usage("invalid dimensions"))?,
        )
        .one()
        .await
        .map_err(pg_error)?;
    if incompatible {
        return Err(AppError::shared(
            "shared embeddings are incomplete or incompatible; run mash index on publishing nodes",
        ));
    }
    let found = candidates(
        &mut tx,
        &input.query,
        &input.vector,
        identity,
        (input.provider, input.days, exclude_node, input.unit),
        input.filter.as_ref(),
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
pub(super) async fn counts(
    tx: &mut Transaction<'_>,
    node: &str,
    identity: &Identity,
) -> Result<Counts, AppError> {
    let row = pg::counts()
        .bind(
            tx,
            &node,
            &identity.generation,
            &i32::try_from(identity.dimensions)
                .map_err(|_| AppError::usage("invalid dimensions"))?,
        )
        .one()
        .await
        .map_err(pg_error)?;
    Ok(Counts {
        conversations: row.conversations.cast_unsigned(),
        messages: row.messages.cast_unsigned(),
        searchable_messages: row.searchable_messages.cast_unsigned(),
        embeddings: row.embeddings.cast_unsigned(),
    })
}
pub(super) async fn status(pool: &Pool, identity: &Identity) -> Result<Counts, AppError> {
    let mut connection = connection(pool).await?;
    let mut tx = transaction(&mut connection, identity).await?;
    counts(&mut tx, &identity.node_id, identity).await
}
pub(super) async fn state(pool: &Pool, identity: &Identity) -> Result<NodeState, AppError> {
    let mut connection = connection(pool).await?;
    let tx = transaction(&mut connection, identity).await?;
    let node = pg::node_state()
        .bind(&tx, &identity.node_id)
        .opt()
        .await
        .map_err(pg_error)?;
    let (instance, revision) = if let Some(row) = node {
        (Some(row.instance), row.revision)
    } else {
        (None, 0)
    };
    let mut sources = Vec::new();
    for r in pg::sources()
        .bind(
            &tx,
            &identity.generation,
            &i32::try_from(identity.dimensions)
                .map_err(|_| AppError::usage("invalid dimensions"))?,
            &identity.node_id,
        )
        .all()
        .await
        .map_err(pg_error)?
    {
        let interpretation_version = u32::try_from(r.interpretation_version)
            .map_err(|_| AppError::shared("invalid stored interpretation version"))?;
        sources.push(Checkpoint {
            provider: r.provider.parse()?,
            path_hash: r.path_hash,
            size_bytes: r.size_bytes,
            modified_ns: r.modified_ns,
            fingerprint: r.fingerprint,
            interpretation_version,
            conversation_id: r.conversation_id,
            ready: r.ready
                && interpretation_version == crate::silver::certificate::INTERPRETATION_VERSION,
        });
    }
    let mut tombstones = Vec::new();
    for r in pg::tombstones()
        .bind(&tx, &identity.node_id)
        .all()
        .await
        .map_err(pg_error)?
    {
        tombstones.push(Tombstone {
            provider: r.provider.parse()?,
            conversation_id: r.conversation_id,
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
