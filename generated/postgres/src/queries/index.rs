// This file was generated with `cornucopia`. Do not modify.

#[derive(Debug)]
pub struct CountsParams<T1: crate::StringSql, T2: crate::StringSql> {
    pub node: T1,
    pub generation: T2,
    pub dimensions: i32,
}
#[derive(Debug)]
pub struct IncompatibleParams<T1: crate::StringSql, T2: crate::StringSql> {
    pub exclude_node: Option<T1>,
    pub generation: T2,
    pub dimensions: i32,
}
#[derive(Debug)]
pub struct ViewParams<T1: crate::StringSql, T2: crate::StringSql> {
    pub node: T1,
    pub id: T2,
    pub context: i64,
}
#[derive(Debug)]
pub struct SourcesParams<T1: crate::StringSql, T2: crate::StringSql> {
    pub generation: T1,
    pub dimensions: i32,
    pub node: T2,
}
#[derive(Debug)]
pub struct SearchParams<
    T1: crate::StringSql,
    T2: crate::StringSql,
    T3: crate::StringSql,
    T4: crate::StringSql,
> {
    pub query: T1,
    pub generation: T2,
    pub provider: Option<T3>,
    pub cutoff: Option<i64>,
    pub exclude_node: Option<T4>,
    pub limit: i64,
}
#[derive(Debug)]
pub struct SemanticParams<
    T1: crate::StringSql,
    T2: crate::StringSql,
    T3: crate::StringSql,
    T4: crate::StringSql,
> {
    pub vector: T1,
    pub generation: T2,
    pub dimensions: i32,
    pub provider: Option<T3>,
    pub cutoff: Option<i64>,
    pub exclude_node: Option<T4>,
    pub limit: i64,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub instance: String,
    pub revision: i64,
}
pub struct NodeBorrowed<'a> {
    pub instance: &'a str,
    pub revision: i64,
}
impl<'a> From<NodeBorrowed<'a>> for Node {
    fn from(NodeBorrowed { instance, revision }: NodeBorrowed<'a>) -> Self {
        Self {
            instance: instance.into(),
            revision,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct NodeState {
    pub instance: String,
    pub revision: i64,
}
pub struct NodeStateBorrowed<'a> {
    pub instance: &'a str,
    pub revision: i64,
}
impl<'a> From<NodeStateBorrowed<'a>> for NodeState {
    fn from(NodeStateBorrowed { instance, revision }: NodeStateBorrowed<'a>) -> Self {
        Self {
            instance: instance.into(),
            revision,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Copy)]
pub struct Counts {
    pub conversations: i64,
    pub messages: i64,
    pub searchable_messages: i64,
    pub embeddings: i64,
}
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub id: String,
    pub ordinal: i64,
    pub role: String,
    pub content: String,
    pub projection: String,
    pub created_at: Option<i64>,
}
pub struct ViewBorrowed<'a> {
    pub id: &'a str,
    pub ordinal: i64,
    pub role: &'a str,
    pub content: &'a str,
    pub projection: &'a str,
    pub created_at: Option<i64>,
}
impl<'a> From<ViewBorrowed<'a>> for View {
    fn from(
        ViewBorrowed {
            id,
            ordinal,
            role,
            content,
            projection,
            created_at,
        }: ViewBorrowed<'a>,
    ) -> Self {
        Self {
            id: id.into(),
            ordinal,
            role: role.into(),
            content: content.into(),
            projection: projection.into(),
            created_at,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct Freshness {
    pub node_id: String,
    pub revision: i64,
    pub last_synced_at: Option<String>,
    pub last_updated_at: Option<String>,
    pub conversations: i64,
    pub messages: i64,
}
pub struct FreshnessBorrowed<'a> {
    pub node_id: &'a str,
    pub revision: i64,
    pub last_synced_at: Option<&'a str>,
    pub last_updated_at: Option<&'a str>,
    pub conversations: i64,
    pub messages: i64,
}
impl<'a> From<FreshnessBorrowed<'a>> for Freshness {
    fn from(
        FreshnessBorrowed {
            node_id,
            revision,
            last_synced_at,
            last_updated_at,
            conversations,
            messages,
        }: FreshnessBorrowed<'a>,
    ) -> Self {
        Self {
            node_id: node_id.into(),
            revision,
            last_synced_at: last_synced_at.map(|v| v.into()),
            last_updated_at: last_updated_at.map(|v| v.into()),
            conversations,
            messages,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct Sources {
    pub provider: String,
    pub path_hash: String,
    pub size_bytes: i64,
    pub modified_ns: i64,
    pub conversation_id: Option<String>,
    pub fingerprint: String,
    pub ready: bool,
}
pub struct SourcesBorrowed<'a> {
    pub provider: &'a str,
    pub path_hash: &'a str,
    pub size_bytes: i64,
    pub modified_ns: i64,
    pub conversation_id: Option<&'a str>,
    pub fingerprint: &'a str,
    pub ready: bool,
}
impl<'a> From<SourcesBorrowed<'a>> for Sources {
    fn from(
        SourcesBorrowed {
            provider,
            path_hash,
            size_bytes,
            modified_ns,
            conversation_id,
            fingerprint,
            ready,
        }: SourcesBorrowed<'a>,
    ) -> Self {
        Self {
            provider: provider.into(),
            path_hash: path_hash.into(),
            size_bytes,
            modified_ns,
            conversation_id: conversation_id.map(|v| v.into()),
            fingerprint: fingerprint.into(),
            ready,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct Tombstones {
    pub provider: String,
    pub conversation_id: String,
}
pub struct TombstonesBorrowed<'a> {
    pub provider: &'a str,
    pub conversation_id: &'a str,
}
impl<'a> From<TombstonesBorrowed<'a>> for Tombstones {
    fn from(
        TombstonesBorrowed {
            provider,
            conversation_id,
        }: TombstonesBorrowed<'a>,
    ) -> Self {
        Self {
            provider: provider.into(),
            conversation_id: conversation_id.into(),
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct Search {
    pub node_id: String,
    pub id: String,
    pub conversation_id: String,
    pub provider: String,
    pub role: String,
    pub projection: String,
    pub score: f64,
    pub created_at: Option<i64>,
    pub title: Option<String>,
    pub project: Option<String>,
    pub repository: Option<String>,
    pub conversation_created_at: Option<i64>,
    pub conversation_updated_at: Option<i64>,
}
pub struct SearchBorrowed<'a> {
    pub node_id: &'a str,
    pub id: &'a str,
    pub conversation_id: &'a str,
    pub provider: &'a str,
    pub role: &'a str,
    pub projection: &'a str,
    pub score: f64,
    pub created_at: Option<i64>,
    pub title: Option<&'a str>,
    pub project: Option<&'a str>,
    pub repository: Option<&'a str>,
    pub conversation_created_at: Option<i64>,
    pub conversation_updated_at: Option<i64>,
}
impl<'a> From<SearchBorrowed<'a>> for Search {
    fn from(
        SearchBorrowed {
            node_id,
            id,
            conversation_id,
            provider,
            role,
            projection,
            score,
            created_at,
            title,
            project,
            repository,
            conversation_created_at,
            conversation_updated_at,
        }: SearchBorrowed<'a>,
    ) -> Self {
        Self {
            node_id: node_id.into(),
            id: id.into(),
            conversation_id: conversation_id.into(),
            provider: provider.into(),
            role: role.into(),
            projection: projection.into(),
            score,
            created_at,
            title: title.map(|v| v.into()),
            project: project.map(|v| v.into()),
            repository: repository.map(|v| v.into()),
            conversation_created_at,
            conversation_updated_at,
        }
    }
}
use crate::client::async_::GenericClient;
use futures::{self, StreamExt, TryStreamExt};
pub struct NodeQuery<'c, 'a, 's, C: GenericClient, T, const N: usize> {
    client: &'c C,
    params: [&'a (dyn postgres_types::ToSql + Sync); N],
    query: &'static str,
    cached: Option<&'s tokio_postgres::Statement>,
    extractor: fn(&tokio_postgres::Row) -> Result<NodeBorrowed, tokio_postgres::Error>,
    mapper: fn(NodeBorrowed) -> T,
}
impl<'c, 'a, 's, C, T: 'c, const N: usize> NodeQuery<'c, 'a, 's, C, T, N>
where
    C: GenericClient,
{
    pub fn map<R>(self, mapper: fn(NodeBorrowed) -> R) -> NodeQuery<'c, 'a, 's, C, R, N> {
        NodeQuery {
            client: self.client,
            params: self.params,
            query: self.query,
            cached: self.cached,
            extractor: self.extractor,
            mapper,
        }
    }
    pub async fn one(self) -> Result<T, tokio_postgres::Error> {
        let row =
            crate::client::async_::one(self.client, self.query, &self.params, self.cached).await?;
        Ok((self.mapper)((self.extractor)(&row)?))
    }
    pub async fn all(self) -> Result<Vec<T>, tokio_postgres::Error> {
        self.iter().await?.try_collect().await
    }
    pub async fn opt(self) -> Result<Option<T>, tokio_postgres::Error> {
        let opt_row =
            crate::client::async_::opt(self.client, self.query, &self.params, self.cached).await?;
        Ok(opt_row
            .map(|row| {
                let extracted = (self.extractor)(&row)?;
                Ok((self.mapper)(extracted))
            })
            .transpose()?)
    }
    pub async fn iter(
        self,
    ) -> Result<
        impl futures::Stream<Item = Result<T, tokio_postgres::Error>> + 'c,
        tokio_postgres::Error,
    > {
        let stream = crate::client::async_::raw(
            self.client,
            self.query,
            crate::slice_iter(&self.params),
            self.cached,
        )
        .await?;
        let mapped = stream
            .map(move |res| {
                res.and_then(|row| {
                    let extracted = (self.extractor)(&row)?;
                    Ok((self.mapper)(extracted))
                })
            })
            .into_stream();
        Ok(mapped)
    }
}
pub struct NodeStateQuery<'c, 'a, 's, C: GenericClient, T, const N: usize> {
    client: &'c C,
    params: [&'a (dyn postgres_types::ToSql + Sync); N],
    query: &'static str,
    cached: Option<&'s tokio_postgres::Statement>,
    extractor: fn(&tokio_postgres::Row) -> Result<NodeStateBorrowed, tokio_postgres::Error>,
    mapper: fn(NodeStateBorrowed) -> T,
}
impl<'c, 'a, 's, C, T: 'c, const N: usize> NodeStateQuery<'c, 'a, 's, C, T, N>
where
    C: GenericClient,
{
    pub fn map<R>(self, mapper: fn(NodeStateBorrowed) -> R) -> NodeStateQuery<'c, 'a, 's, C, R, N> {
        NodeStateQuery {
            client: self.client,
            params: self.params,
            query: self.query,
            cached: self.cached,
            extractor: self.extractor,
            mapper,
        }
    }
    pub async fn one(self) -> Result<T, tokio_postgres::Error> {
        let row =
            crate::client::async_::one(self.client, self.query, &self.params, self.cached).await?;
        Ok((self.mapper)((self.extractor)(&row)?))
    }
    pub async fn all(self) -> Result<Vec<T>, tokio_postgres::Error> {
        self.iter().await?.try_collect().await
    }
    pub async fn opt(self) -> Result<Option<T>, tokio_postgres::Error> {
        let opt_row =
            crate::client::async_::opt(self.client, self.query, &self.params, self.cached).await?;
        Ok(opt_row
            .map(|row| {
                let extracted = (self.extractor)(&row)?;
                Ok((self.mapper)(extracted))
            })
            .transpose()?)
    }
    pub async fn iter(
        self,
    ) -> Result<
        impl futures::Stream<Item = Result<T, tokio_postgres::Error>> + 'c,
        tokio_postgres::Error,
    > {
        let stream = crate::client::async_::raw(
            self.client,
            self.query,
            crate::slice_iter(&self.params),
            self.cached,
        )
        .await?;
        let mapped = stream
            .map(move |res| {
                res.and_then(|row| {
                    let extracted = (self.extractor)(&row)?;
                    Ok((self.mapper)(extracted))
                })
            })
            .into_stream();
        Ok(mapped)
    }
}
pub struct I32Query<'c, 'a, 's, C: GenericClient, T, const N: usize> {
    client: &'c C,
    params: [&'a (dyn postgres_types::ToSql + Sync); N],
    query: &'static str,
    cached: Option<&'s tokio_postgres::Statement>,
    extractor: fn(&tokio_postgres::Row) -> Result<i32, tokio_postgres::Error>,
    mapper: fn(i32) -> T,
}
impl<'c, 'a, 's, C, T: 'c, const N: usize> I32Query<'c, 'a, 's, C, T, N>
where
    C: GenericClient,
{
    pub fn map<R>(self, mapper: fn(i32) -> R) -> I32Query<'c, 'a, 's, C, R, N> {
        I32Query {
            client: self.client,
            params: self.params,
            query: self.query,
            cached: self.cached,
            extractor: self.extractor,
            mapper,
        }
    }
    pub async fn one(self) -> Result<T, tokio_postgres::Error> {
        let row =
            crate::client::async_::one(self.client, self.query, &self.params, self.cached).await?;
        Ok((self.mapper)((self.extractor)(&row)?))
    }
    pub async fn all(self) -> Result<Vec<T>, tokio_postgres::Error> {
        self.iter().await?.try_collect().await
    }
    pub async fn opt(self) -> Result<Option<T>, tokio_postgres::Error> {
        let opt_row =
            crate::client::async_::opt(self.client, self.query, &self.params, self.cached).await?;
        Ok(opt_row
            .map(|row| {
                let extracted = (self.extractor)(&row)?;
                Ok((self.mapper)(extracted))
            })
            .transpose()?)
    }
    pub async fn iter(
        self,
    ) -> Result<
        impl futures::Stream<Item = Result<T, tokio_postgres::Error>> + 'c,
        tokio_postgres::Error,
    > {
        let stream = crate::client::async_::raw(
            self.client,
            self.query,
            crate::slice_iter(&self.params),
            self.cached,
        )
        .await?;
        let mapped = stream
            .map(move |res| {
                res.and_then(|row| {
                    let extracted = (self.extractor)(&row)?;
                    Ok((self.mapper)(extracted))
                })
            })
            .into_stream();
        Ok(mapped)
    }
}
pub struct CountsQuery<'c, 'a, 's, C: GenericClient, T, const N: usize> {
    client: &'c C,
    params: [&'a (dyn postgres_types::ToSql + Sync); N],
    query: &'static str,
    cached: Option<&'s tokio_postgres::Statement>,
    extractor: fn(&tokio_postgres::Row) -> Result<Counts, tokio_postgres::Error>,
    mapper: fn(Counts) -> T,
}
impl<'c, 'a, 's, C, T: 'c, const N: usize> CountsQuery<'c, 'a, 's, C, T, N>
where
    C: GenericClient,
{
    pub fn map<R>(self, mapper: fn(Counts) -> R) -> CountsQuery<'c, 'a, 's, C, R, N> {
        CountsQuery {
            client: self.client,
            params: self.params,
            query: self.query,
            cached: self.cached,
            extractor: self.extractor,
            mapper,
        }
    }
    pub async fn one(self) -> Result<T, tokio_postgres::Error> {
        let row =
            crate::client::async_::one(self.client, self.query, &self.params, self.cached).await?;
        Ok((self.mapper)((self.extractor)(&row)?))
    }
    pub async fn all(self) -> Result<Vec<T>, tokio_postgres::Error> {
        self.iter().await?.try_collect().await
    }
    pub async fn opt(self) -> Result<Option<T>, tokio_postgres::Error> {
        let opt_row =
            crate::client::async_::opt(self.client, self.query, &self.params, self.cached).await?;
        Ok(opt_row
            .map(|row| {
                let extracted = (self.extractor)(&row)?;
                Ok((self.mapper)(extracted))
            })
            .transpose()?)
    }
    pub async fn iter(
        self,
    ) -> Result<
        impl futures::Stream<Item = Result<T, tokio_postgres::Error>> + 'c,
        tokio_postgres::Error,
    > {
        let stream = crate::client::async_::raw(
            self.client,
            self.query,
            crate::slice_iter(&self.params),
            self.cached,
        )
        .await?;
        let mapped = stream
            .map(move |res| {
                res.and_then(|row| {
                    let extracted = (self.extractor)(&row)?;
                    Ok((self.mapper)(extracted))
                })
            })
            .into_stream();
        Ok(mapped)
    }
}
pub struct BoolQuery<'c, 'a, 's, C: GenericClient, T, const N: usize> {
    client: &'c C,
    params: [&'a (dyn postgres_types::ToSql + Sync); N],
    query: &'static str,
    cached: Option<&'s tokio_postgres::Statement>,
    extractor: fn(&tokio_postgres::Row) -> Result<bool, tokio_postgres::Error>,
    mapper: fn(bool) -> T,
}
impl<'c, 'a, 's, C, T: 'c, const N: usize> BoolQuery<'c, 'a, 's, C, T, N>
where
    C: GenericClient,
{
    pub fn map<R>(self, mapper: fn(bool) -> R) -> BoolQuery<'c, 'a, 's, C, R, N> {
        BoolQuery {
            client: self.client,
            params: self.params,
            query: self.query,
            cached: self.cached,
            extractor: self.extractor,
            mapper,
        }
    }
    pub async fn one(self) -> Result<T, tokio_postgres::Error> {
        let row =
            crate::client::async_::one(self.client, self.query, &self.params, self.cached).await?;
        Ok((self.mapper)((self.extractor)(&row)?))
    }
    pub async fn all(self) -> Result<Vec<T>, tokio_postgres::Error> {
        self.iter().await?.try_collect().await
    }
    pub async fn opt(self) -> Result<Option<T>, tokio_postgres::Error> {
        let opt_row =
            crate::client::async_::opt(self.client, self.query, &self.params, self.cached).await?;
        Ok(opt_row
            .map(|row| {
                let extracted = (self.extractor)(&row)?;
                Ok((self.mapper)(extracted))
            })
            .transpose()?)
    }
    pub async fn iter(
        self,
    ) -> Result<
        impl futures::Stream<Item = Result<T, tokio_postgres::Error>> + 'c,
        tokio_postgres::Error,
    > {
        let stream = crate::client::async_::raw(
            self.client,
            self.query,
            crate::slice_iter(&self.params),
            self.cached,
        )
        .await?;
        let mapped = stream
            .map(move |res| {
                res.and_then(|row| {
                    let extracted = (self.extractor)(&row)?;
                    Ok((self.mapper)(extracted))
                })
            })
            .into_stream();
        Ok(mapped)
    }
}
pub struct ViewQuery<'c, 'a, 's, C: GenericClient, T, const N: usize> {
    client: &'c C,
    params: [&'a (dyn postgres_types::ToSql + Sync); N],
    query: &'static str,
    cached: Option<&'s tokio_postgres::Statement>,
    extractor: fn(&tokio_postgres::Row) -> Result<ViewBorrowed, tokio_postgres::Error>,
    mapper: fn(ViewBorrowed) -> T,
}
impl<'c, 'a, 's, C, T: 'c, const N: usize> ViewQuery<'c, 'a, 's, C, T, N>
where
    C: GenericClient,
{
    pub fn map<R>(self, mapper: fn(ViewBorrowed) -> R) -> ViewQuery<'c, 'a, 's, C, R, N> {
        ViewQuery {
            client: self.client,
            params: self.params,
            query: self.query,
            cached: self.cached,
            extractor: self.extractor,
            mapper,
        }
    }
    pub async fn one(self) -> Result<T, tokio_postgres::Error> {
        let row =
            crate::client::async_::one(self.client, self.query, &self.params, self.cached).await?;
        Ok((self.mapper)((self.extractor)(&row)?))
    }
    pub async fn all(self) -> Result<Vec<T>, tokio_postgres::Error> {
        self.iter().await?.try_collect().await
    }
    pub async fn opt(self) -> Result<Option<T>, tokio_postgres::Error> {
        let opt_row =
            crate::client::async_::opt(self.client, self.query, &self.params, self.cached).await?;
        Ok(opt_row
            .map(|row| {
                let extracted = (self.extractor)(&row)?;
                Ok((self.mapper)(extracted))
            })
            .transpose()?)
    }
    pub async fn iter(
        self,
    ) -> Result<
        impl futures::Stream<Item = Result<T, tokio_postgres::Error>> + 'c,
        tokio_postgres::Error,
    > {
        let stream = crate::client::async_::raw(
            self.client,
            self.query,
            crate::slice_iter(&self.params),
            self.cached,
        )
        .await?;
        let mapped = stream
            .map(move |res| {
                res.and_then(|row| {
                    let extracted = (self.extractor)(&row)?;
                    Ok((self.mapper)(extracted))
                })
            })
            .into_stream();
        Ok(mapped)
    }
}
pub struct FreshnessQuery<'c, 'a, 's, C: GenericClient, T, const N: usize> {
    client: &'c C,
    params: [&'a (dyn postgres_types::ToSql + Sync); N],
    query: &'static str,
    cached: Option<&'s tokio_postgres::Statement>,
    extractor: fn(&tokio_postgres::Row) -> Result<FreshnessBorrowed, tokio_postgres::Error>,
    mapper: fn(FreshnessBorrowed) -> T,
}
impl<'c, 'a, 's, C, T: 'c, const N: usize> FreshnessQuery<'c, 'a, 's, C, T, N>
where
    C: GenericClient,
{
    pub fn map<R>(self, mapper: fn(FreshnessBorrowed) -> R) -> FreshnessQuery<'c, 'a, 's, C, R, N> {
        FreshnessQuery {
            client: self.client,
            params: self.params,
            query: self.query,
            cached: self.cached,
            extractor: self.extractor,
            mapper,
        }
    }
    pub async fn one(self) -> Result<T, tokio_postgres::Error> {
        let row =
            crate::client::async_::one(self.client, self.query, &self.params, self.cached).await?;
        Ok((self.mapper)((self.extractor)(&row)?))
    }
    pub async fn all(self) -> Result<Vec<T>, tokio_postgres::Error> {
        self.iter().await?.try_collect().await
    }
    pub async fn opt(self) -> Result<Option<T>, tokio_postgres::Error> {
        let opt_row =
            crate::client::async_::opt(self.client, self.query, &self.params, self.cached).await?;
        Ok(opt_row
            .map(|row| {
                let extracted = (self.extractor)(&row)?;
                Ok((self.mapper)(extracted))
            })
            .transpose()?)
    }
    pub async fn iter(
        self,
    ) -> Result<
        impl futures::Stream<Item = Result<T, tokio_postgres::Error>> + 'c,
        tokio_postgres::Error,
    > {
        let stream = crate::client::async_::raw(
            self.client,
            self.query,
            crate::slice_iter(&self.params),
            self.cached,
        )
        .await?;
        let mapped = stream
            .map(move |res| {
                res.and_then(|row| {
                    let extracted = (self.extractor)(&row)?;
                    Ok((self.mapper)(extracted))
                })
            })
            .into_stream();
        Ok(mapped)
    }
}
pub struct SourcesQuery<'c, 'a, 's, C: GenericClient, T, const N: usize> {
    client: &'c C,
    params: [&'a (dyn postgres_types::ToSql + Sync); N],
    query: &'static str,
    cached: Option<&'s tokio_postgres::Statement>,
    extractor: fn(&tokio_postgres::Row) -> Result<SourcesBorrowed, tokio_postgres::Error>,
    mapper: fn(SourcesBorrowed) -> T,
}
impl<'c, 'a, 's, C, T: 'c, const N: usize> SourcesQuery<'c, 'a, 's, C, T, N>
where
    C: GenericClient,
{
    pub fn map<R>(self, mapper: fn(SourcesBorrowed) -> R) -> SourcesQuery<'c, 'a, 's, C, R, N> {
        SourcesQuery {
            client: self.client,
            params: self.params,
            query: self.query,
            cached: self.cached,
            extractor: self.extractor,
            mapper,
        }
    }
    pub async fn one(self) -> Result<T, tokio_postgres::Error> {
        let row =
            crate::client::async_::one(self.client, self.query, &self.params, self.cached).await?;
        Ok((self.mapper)((self.extractor)(&row)?))
    }
    pub async fn all(self) -> Result<Vec<T>, tokio_postgres::Error> {
        self.iter().await?.try_collect().await
    }
    pub async fn opt(self) -> Result<Option<T>, tokio_postgres::Error> {
        let opt_row =
            crate::client::async_::opt(self.client, self.query, &self.params, self.cached).await?;
        Ok(opt_row
            .map(|row| {
                let extracted = (self.extractor)(&row)?;
                Ok((self.mapper)(extracted))
            })
            .transpose()?)
    }
    pub async fn iter(
        self,
    ) -> Result<
        impl futures::Stream<Item = Result<T, tokio_postgres::Error>> + 'c,
        tokio_postgres::Error,
    > {
        let stream = crate::client::async_::raw(
            self.client,
            self.query,
            crate::slice_iter(&self.params),
            self.cached,
        )
        .await?;
        let mapped = stream
            .map(move |res| {
                res.and_then(|row| {
                    let extracted = (self.extractor)(&row)?;
                    Ok((self.mapper)(extracted))
                })
            })
            .into_stream();
        Ok(mapped)
    }
}
pub struct TombstonesQuery<'c, 'a, 's, C: GenericClient, T, const N: usize> {
    client: &'c C,
    params: [&'a (dyn postgres_types::ToSql + Sync); N],
    query: &'static str,
    cached: Option<&'s tokio_postgres::Statement>,
    extractor: fn(&tokio_postgres::Row) -> Result<TombstonesBorrowed, tokio_postgres::Error>,
    mapper: fn(TombstonesBorrowed) -> T,
}
impl<'c, 'a, 's, C, T: 'c, const N: usize> TombstonesQuery<'c, 'a, 's, C, T, N>
where
    C: GenericClient,
{
    pub fn map<R>(
        self,
        mapper: fn(TombstonesBorrowed) -> R,
    ) -> TombstonesQuery<'c, 'a, 's, C, R, N> {
        TombstonesQuery {
            client: self.client,
            params: self.params,
            query: self.query,
            cached: self.cached,
            extractor: self.extractor,
            mapper,
        }
    }
    pub async fn one(self) -> Result<T, tokio_postgres::Error> {
        let row =
            crate::client::async_::one(self.client, self.query, &self.params, self.cached).await?;
        Ok((self.mapper)((self.extractor)(&row)?))
    }
    pub async fn all(self) -> Result<Vec<T>, tokio_postgres::Error> {
        self.iter().await?.try_collect().await
    }
    pub async fn opt(self) -> Result<Option<T>, tokio_postgres::Error> {
        let opt_row =
            crate::client::async_::opt(self.client, self.query, &self.params, self.cached).await?;
        Ok(opt_row
            .map(|row| {
                let extracted = (self.extractor)(&row)?;
                Ok((self.mapper)(extracted))
            })
            .transpose()?)
    }
    pub async fn iter(
        self,
    ) -> Result<
        impl futures::Stream<Item = Result<T, tokio_postgres::Error>> + 'c,
        tokio_postgres::Error,
    > {
        let stream = crate::client::async_::raw(
            self.client,
            self.query,
            crate::slice_iter(&self.params),
            self.cached,
        )
        .await?;
        let mapped = stream
            .map(move |res| {
                res.and_then(|row| {
                    let extracted = (self.extractor)(&row)?;
                    Ok((self.mapper)(extracted))
                })
            })
            .into_stream();
        Ok(mapped)
    }
}
pub struct SearchQuery<'c, 'a, 's, C: GenericClient, T, const N: usize> {
    client: &'c C,
    params: [&'a (dyn postgres_types::ToSql + Sync); N],
    query: &'static str,
    cached: Option<&'s tokio_postgres::Statement>,
    extractor: fn(&tokio_postgres::Row) -> Result<SearchBorrowed, tokio_postgres::Error>,
    mapper: fn(SearchBorrowed) -> T,
}
impl<'c, 'a, 's, C, T: 'c, const N: usize> SearchQuery<'c, 'a, 's, C, T, N>
where
    C: GenericClient,
{
    pub fn map<R>(self, mapper: fn(SearchBorrowed) -> R) -> SearchQuery<'c, 'a, 's, C, R, N> {
        SearchQuery {
            client: self.client,
            params: self.params,
            query: self.query,
            cached: self.cached,
            extractor: self.extractor,
            mapper,
        }
    }
    pub async fn one(self) -> Result<T, tokio_postgres::Error> {
        let row =
            crate::client::async_::one(self.client, self.query, &self.params, self.cached).await?;
        Ok((self.mapper)((self.extractor)(&row)?))
    }
    pub async fn all(self) -> Result<Vec<T>, tokio_postgres::Error> {
        self.iter().await?.try_collect().await
    }
    pub async fn opt(self) -> Result<Option<T>, tokio_postgres::Error> {
        let opt_row =
            crate::client::async_::opt(self.client, self.query, &self.params, self.cached).await?;
        Ok(opt_row
            .map(|row| {
                let extracted = (self.extractor)(&row)?;
                Ok((self.mapper)(extracted))
            })
            .transpose()?)
    }
    pub async fn iter(
        self,
    ) -> Result<
        impl futures::Stream<Item = Result<T, tokio_postgres::Error>> + 'c,
        tokio_postgres::Error,
    > {
        let stream = crate::client::async_::raw(
            self.client,
            self.query,
            crate::slice_iter(&self.params),
            self.cached,
        )
        .await?;
        let mapped = stream
            .map(move |res| {
                res.and_then(|row| {
                    let extracted = (self.extractor)(&row)?;
                    Ok((self.mapper)(extracted))
                })
            })
            .into_stream();
        Ok(mapped)
    }
}
pub struct NodeStmt(&'static str, Option<tokio_postgres::Statement>);
pub fn node() -> NodeStmt {
    NodeStmt(
        "SELECT instance, revision FROM nodes WHERE node_id=$1 FOR UPDATE",
        None,
    )
}
impl NodeStmt {
    pub async fn prepare<'a, C: GenericClient>(
        mut self,
        client: &'a C,
    ) -> Result<Self, tokio_postgres::Error> {
        self.1 = Some(client.prepare(self.0).await?);
        Ok(self)
    }
    pub fn bind<'c, 'a, 's, C: GenericClient, T1: crate::StringSql>(
        &'s self,
        client: &'c C,
        node: &'a T1,
    ) -> NodeQuery<'c, 'a, 's, C, Node, 1> {
        NodeQuery {
            client,
            params: [node],
            query: self.0,
            cached: self.1.as_ref(),
            extractor: |row: &tokio_postgres::Row| -> Result<NodeBorrowed, tokio_postgres::Error> {
                Ok(NodeBorrowed {
                    instance: row.try_get(0)?,
                    revision: row.try_get(1)?,
                })
            },
            mapper: |it| Node::from(it),
        }
    }
}
pub struct NodeStateStmt(&'static str, Option<tokio_postgres::Statement>);
pub fn node_state() -> NodeStateStmt {
    NodeStateStmt(
        "SELECT instance, revision FROM nodes WHERE node_id=$1",
        None,
    )
}
impl NodeStateStmt {
    pub async fn prepare<'a, C: GenericClient>(
        mut self,
        client: &'a C,
    ) -> Result<Self, tokio_postgres::Error> {
        self.1 = Some(client.prepare(self.0).await?);
        Ok(self)
    }
    pub fn bind<'c, 'a, 's, C: GenericClient, T1: crate::StringSql>(
        &'s self,
        client: &'c C,
        node: &'a T1,
    ) -> NodeStateQuery<'c, 'a, 's, C, NodeState, 1> {
        NodeStateQuery {
            client,
            params: [node],
            query: self.0,
            cached: self.1.as_ref(),
            extractor:
                |row: &tokio_postgres::Row| -> Result<NodeStateBorrowed, tokio_postgres::Error> {
                    Ok(NodeStateBorrowed {
                        instance: row.try_get(0)?,
                        revision: row.try_get(1)?,
                    })
                },
            mapper: |it| NodeState::from(it),
        }
    }
}
pub struct VersionStmt(&'static str, Option<tokio_postgres::Statement>);
pub fn version() -> VersionStmt {
    VersionStmt("SELECT version FROM version WHERE singleton", None)
}
impl VersionStmt {
    pub async fn prepare<'a, C: GenericClient>(
        mut self,
        client: &'a C,
    ) -> Result<Self, tokio_postgres::Error> {
        self.1 = Some(client.prepare(self.0).await?);
        Ok(self)
    }
    pub fn bind<'c, 'a, 's, C: GenericClient>(
        &'s self,
        client: &'c C,
    ) -> I32Query<'c, 'a, 's, C, i32, 0> {
        I32Query {
            client,
            params: [],
            query: self.0,
            cached: self.1.as_ref(),
            extractor: |row| Ok(row.try_get(0)?),
            mapper: |it| it,
        }
    }
}
pub struct CountsStmt(&'static str, Option<tokio_postgres::Statement>);
pub fn counts() -> CountsStmt {
    CountsStmt(
        "SELECT (SELECT count(*) FROM conversations WHERE node_id=$1) AS conversations, count(*) AS messages, count(*) FILTER(WHERE projection<>'') AS searchable_messages, count(*) FILTER(WHERE projection<>'' AND generation=$2 AND vector_dims(vector)=$3) AS embeddings FROM messages WHERE node_id=$1",
        None,
    )
}
impl CountsStmt {
    pub async fn prepare<'a, C: GenericClient>(
        mut self,
        client: &'a C,
    ) -> Result<Self, tokio_postgres::Error> {
        self.1 = Some(client.prepare(self.0).await?);
        Ok(self)
    }
    pub fn bind<'c, 'a, 's, C: GenericClient, T1: crate::StringSql, T2: crate::StringSql>(
        &'s self,
        client: &'c C,
        node: &'a T1,
        generation: &'a T2,
        dimensions: &'a i32,
    ) -> CountsQuery<'c, 'a, 's, C, Counts, 3> {
        CountsQuery {
            client,
            params: [node, generation, dimensions],
            query: self.0,
            cached: self.1.as_ref(),
            extractor: |row: &tokio_postgres::Row| -> Result<Counts, tokio_postgres::Error> {
                Ok(Counts {
                    conversations: row.try_get(0)?,
                    messages: row.try_get(1)?,
                    searchable_messages: row.try_get(2)?,
                    embeddings: row.try_get(3)?,
                })
            },
            mapper: |it| Counts::from(it),
        }
    }
}
impl<'c, 'a, 's, C: GenericClient, T1: crate::StringSql, T2: crate::StringSql>
    crate::client::async_::Params<
        'c,
        'a,
        's,
        CountsParams<T1, T2>,
        CountsQuery<'c, 'a, 's, C, Counts, 3>,
        C,
    > for CountsStmt
{
    fn params(
        &'s self,
        client: &'c C,
        params: &'a CountsParams<T1, T2>,
    ) -> CountsQuery<'c, 'a, 's, C, Counts, 3> {
        self.bind(client, &params.node, &params.generation, &params.dimensions)
    }
}
pub struct IncompatibleStmt(&'static str, Option<tokio_postgres::Statement>);
pub fn incompatible() -> IncompatibleStmt {
    IncompatibleStmt(
        "SELECT EXISTS(SELECT 1 FROM messages WHERE projection<>'' AND ($1::text IS NULL OR node_id<>$1) AND (generation IS DISTINCT FROM $2 OR vector IS NULL OR vector_dims(vector)<>$3)) AS incompatible",
        None,
    )
}
impl IncompatibleStmt {
    pub async fn prepare<'a, C: GenericClient>(
        mut self,
        client: &'a C,
    ) -> Result<Self, tokio_postgres::Error> {
        self.1 = Some(client.prepare(self.0).await?);
        Ok(self)
    }
    pub fn bind<'c, 'a, 's, C: GenericClient, T1: crate::StringSql, T2: crate::StringSql>(
        &'s self,
        client: &'c C,
        exclude_node: &'a Option<T1>,
        generation: &'a T2,
        dimensions: &'a i32,
    ) -> BoolQuery<'c, 'a, 's, C, bool, 3> {
        BoolQuery {
            client,
            params: [exclude_node, generation, dimensions],
            query: self.0,
            cached: self.1.as_ref(),
            extractor: |row| Ok(row.try_get(0)?),
            mapper: |it| it,
        }
    }
}
impl<'c, 'a, 's, C: GenericClient, T1: crate::StringSql, T2: crate::StringSql>
    crate::client::async_::Params<
        'c,
        'a,
        's,
        IncompatibleParams<T1, T2>,
        BoolQuery<'c, 'a, 's, C, bool, 3>,
        C,
    > for IncompatibleStmt
{
    fn params(
        &'s self,
        client: &'c C,
        params: &'a IncompatibleParams<T1, T2>,
    ) -> BoolQuery<'c, 'a, 's, C, bool, 3> {
        self.bind(
            client,
            &params.exclude_node,
            &params.generation,
            &params.dimensions,
        )
    }
}
pub struct ViewStmt(&'static str, Option<tokio_postgres::Statement>);
pub fn view() -> ViewStmt {
    ViewStmt(
        "WITH RECURSIVE anchor AS (SELECT * FROM messages WHERE node_id=$1 AND id=$2), ancestors(ordinal,depth) AS ( SELECT ordinal,0::bigint FROM anchor WHERE context_parent IS NOT NULL UNION ALL SELECT m.context_parent,a.depth+1 FROM messages m JOIN ancestors a ON m.ordinal=a.ordinal JOIN anchor ON m.node_id=anchor.node_id AND m.conversation_id=anchor.conversation_id WHERE m.context_parent>=0 AND a.depth<$3 ), descendants(ordinal,depth) AS ( SELECT ordinal,0::bigint FROM anchor WHERE context_parent IS NOT NULL UNION ALL SELECT (SELECT MIN(m.ordinal) FROM messages m WHERE m.node_id=anchor.node_id AND m.conversation_id=anchor.conversation_id AND m.context_parent=d.ordinal),d.depth+1 FROM descendants d CROSS JOIN anchor WHERE d.depth<$3 AND EXISTS(SELECT 1 FROM messages m WHERE m.node_id=anchor.node_id AND m.conversation_id=anchor.conversation_id AND m.context_parent=d.ordinal) ) SELECT m.id,m.ordinal,m.role,m.content,m.projection,m.created_at FROM messages m JOIN anchor ON m.node_id=anchor.node_id AND m.conversation_id=anchor.conversation_id WHERE anchor.context_parent IS NULL AND m.ordinal BETWEEN anchor.ordinal-$3 AND anchor.ordinal+$3 UNION ALL SELECT m.id,m.ordinal,m.role,m.content,m.projection,m.created_at FROM messages m JOIN anchor ON m.node_id=anchor.node_id AND m.conversation_id=anchor.conversation_id WHERE m.ordinal IN (SELECT ordinal FROM ancestors UNION SELECT ordinal FROM descendants) ORDER BY ordinal",
        None,
    )
}
impl ViewStmt {
    pub async fn prepare<'a, C: GenericClient>(
        mut self,
        client: &'a C,
    ) -> Result<Self, tokio_postgres::Error> {
        self.1 = Some(client.prepare(self.0).await?);
        Ok(self)
    }
    pub fn bind<'c, 'a, 's, C: GenericClient, T1: crate::StringSql, T2: crate::StringSql>(
        &'s self,
        client: &'c C,
        node: &'a T1,
        id: &'a T2,
        context: &'a i64,
    ) -> ViewQuery<'c, 'a, 's, C, View, 3> {
        ViewQuery {
            client,
            params: [node, id, context],
            query: self.0,
            cached: self.1.as_ref(),
            extractor: |row: &tokio_postgres::Row| -> Result<ViewBorrowed, tokio_postgres::Error> {
                Ok(ViewBorrowed {
                    id: row.try_get(0)?,
                    ordinal: row.try_get(1)?,
                    role: row.try_get(2)?,
                    content: row.try_get(3)?,
                    projection: row.try_get(4)?,
                    created_at: row.try_get(5)?,
                })
            },
            mapper: |it| View::from(it),
        }
    }
}
impl<'c, 'a, 's, C: GenericClient, T1: crate::StringSql, T2: crate::StringSql>
    crate::client::async_::Params<
        'c,
        'a,
        's,
        ViewParams<T1, T2>,
        ViewQuery<'c, 'a, 's, C, View, 3>,
        C,
    > for ViewStmt
{
    fn params(
        &'s self,
        client: &'c C,
        params: &'a ViewParams<T1, T2>,
    ) -> ViewQuery<'c, 'a, 's, C, View, 3> {
        self.bind(client, &params.node, &params.id, &params.context)
    }
}
pub struct FreshnessStmt(&'static str, Option<tokio_postgres::Statement>);
pub fn freshness() -> FreshnessStmt {
    FreshnessStmt(
        "SELECT n.node_id,n.revision, to_char(s.last_synced_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS last_synced_at, to_char(s.last_updated_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS last_updated_at, (SELECT count(*) FROM conversations c WHERE c.node_id=n.node_id) AS conversations, (SELECT count(*) FROM messages m WHERE m.node_id=n.node_id) AS messages FROM nodes n LEFT JOIN node_sync s USING(node_id) ORDER BY n.node_id",
        None,
    )
}
impl FreshnessStmt {
    pub async fn prepare<'a, C: GenericClient>(
        mut self,
        client: &'a C,
    ) -> Result<Self, tokio_postgres::Error> {
        self.1 = Some(client.prepare(self.0).await?);
        Ok(self)
    }
    pub fn bind<'c, 'a, 's, C: GenericClient>(
        &'s self,
        client: &'c C,
    ) -> FreshnessQuery<'c, 'a, 's, C, Freshness, 0> {
        FreshnessQuery {
            client,
            params: [],
            query: self.0,
            cached: self.1.as_ref(),
            extractor:
                |row: &tokio_postgres::Row| -> Result<FreshnessBorrowed, tokio_postgres::Error> {
                    Ok(FreshnessBorrowed {
                        node_id: row.try_get(0)?,
                        revision: row.try_get(1)?,
                        last_synced_at: row.try_get(2)?,
                        last_updated_at: row.try_get(3)?,
                        conversations: row.try_get(4)?,
                        messages: row.try_get(5)?,
                    })
                },
            mapper: |it| Freshness::from(it),
        }
    }
}
pub struct SourcesStmt(&'static str, Option<tokio_postgres::Statement>);
pub fn sources() -> SourcesStmt {
    SourcesStmt(
        "SELECT provider,path_hash,size_bytes,modified_ns,conversation_id,fingerprint, NOT EXISTS(SELECT 1 FROM messages m WHERE m.node_id=s.node_id AND m.conversation_id=s.conversation_id AND projection<>'' AND (generation IS DISTINCT FROM $1 OR vector IS NULL OR vector_dims(vector)<>$2)) AS ready FROM direct_sources s WHERE node_id=$3",
        None,
    )
}
impl SourcesStmt {
    pub async fn prepare<'a, C: GenericClient>(
        mut self,
        client: &'a C,
    ) -> Result<Self, tokio_postgres::Error> {
        self.1 = Some(client.prepare(self.0).await?);
        Ok(self)
    }
    pub fn bind<'c, 'a, 's, C: GenericClient, T1: crate::StringSql, T2: crate::StringSql>(
        &'s self,
        client: &'c C,
        generation: &'a T1,
        dimensions: &'a i32,
        node: &'a T2,
    ) -> SourcesQuery<'c, 'a, 's, C, Sources, 3> {
        SourcesQuery {
            client,
            params: [generation, dimensions, node],
            query: self.0,
            cached: self.1.as_ref(),
            extractor:
                |row: &tokio_postgres::Row| -> Result<SourcesBorrowed, tokio_postgres::Error> {
                    Ok(SourcesBorrowed {
                        provider: row.try_get(0)?,
                        path_hash: row.try_get(1)?,
                        size_bytes: row.try_get(2)?,
                        modified_ns: row.try_get(3)?,
                        conversation_id: row.try_get(4)?,
                        fingerprint: row.try_get(5)?,
                        ready: row.try_get(6)?,
                    })
                },
            mapper: |it| Sources::from(it),
        }
    }
}
impl<'c, 'a, 's, C: GenericClient, T1: crate::StringSql, T2: crate::StringSql>
    crate::client::async_::Params<
        'c,
        'a,
        's,
        SourcesParams<T1, T2>,
        SourcesQuery<'c, 'a, 's, C, Sources, 3>,
        C,
    > for SourcesStmt
{
    fn params(
        &'s self,
        client: &'c C,
        params: &'a SourcesParams<T1, T2>,
    ) -> SourcesQuery<'c, 'a, 's, C, Sources, 3> {
        self.bind(client, &params.generation, &params.dimensions, &params.node)
    }
}
pub struct TombstonesStmt(&'static str, Option<tokio_postgres::Statement>);
pub fn tombstones() -> TombstonesStmt {
    TombstonesStmt(
        "SELECT provider,conversation_id FROM tombstones WHERE node_id=$1",
        None,
    )
}
impl TombstonesStmt {
    pub async fn prepare<'a, C: GenericClient>(
        mut self,
        client: &'a C,
    ) -> Result<Self, tokio_postgres::Error> {
        self.1 = Some(client.prepare(self.0).await?);
        Ok(self)
    }
    pub fn bind<'c, 'a, 's, C: GenericClient, T1: crate::StringSql>(
        &'s self,
        client: &'c C,
        node: &'a T1,
    ) -> TombstonesQuery<'c, 'a, 's, C, Tombstones, 1> {
        TombstonesQuery {
            client,
            params: [node],
            query: self.0,
            cached: self.1.as_ref(),
            extractor:
                |row: &tokio_postgres::Row| -> Result<TombstonesBorrowed, tokio_postgres::Error> {
                    Ok(TombstonesBorrowed {
                        provider: row.try_get(0)?,
                        conversation_id: row.try_get(1)?,
                    })
                },
            mapper: |it| Tombstones::from(it),
        }
    }
}
pub struct BrokenContextStmt(&'static str, Option<tokio_postgres::Statement>);
pub fn broken_context() -> BrokenContextStmt {
    BrokenContextStmt(
        "SELECT EXISTS(SELECT 1 FROM messages m WHERE m.node_id=$1 AND m.context_parent>=0 AND NOT EXISTS(SELECT 1 FROM messages p WHERE p.node_id=m.node_id AND p.conversation_id=m.conversation_id AND p.ordinal=m.context_parent)) AS broken",
        None,
    )
}
impl BrokenContextStmt {
    pub async fn prepare<'a, C: GenericClient>(
        mut self,
        client: &'a C,
    ) -> Result<Self, tokio_postgres::Error> {
        self.1 = Some(client.prepare(self.0).await?);
        Ok(self)
    }
    pub fn bind<'c, 'a, 's, C: GenericClient, T1: crate::StringSql>(
        &'s self,
        client: &'c C,
        node: &'a T1,
    ) -> BoolQuery<'c, 'a, 's, C, bool, 1> {
        BoolQuery {
            client,
            params: [node],
            query: self.0,
            cached: self.1.as_ref(),
            extractor: |row| Ok(row.try_get(0)?),
            mapper: |it| it,
        }
    }
}
pub struct SearchStmt(&'static str, Option<tokio_postgres::Statement>);
pub fn search() -> SearchStmt {
    SearchStmt(
        "SELECT m.node_id,m.id,m.conversation_id,c.provider,m.role,m.projection, ts_rank_cd(document,websearch_to_tsquery('simple',$1))::double precision AS score, m.created_at,c.title,c.project,c.repository,c.created_at AS conversation_created_at,c.updated_at AS conversation_updated_at FROM messages m JOIN conversations c ON c.node_id=m.node_id AND c.id=m.conversation_id WHERE document @@ websearch_to_tsquery('simple',$1) AND projection<>'' AND generation=$2 AND ($3::text IS NULL OR c.provider=$3) AND ($4::bigint IS NULL OR m.created_at>=$4) AND ($5::text IS NULL OR m.node_id<>$5) ORDER BY score DESC,m.node_id,m.id LIMIT $6",
        None,
    )
}
impl SearchStmt {
    pub async fn prepare<'a, C: GenericClient>(
        mut self,
        client: &'a C,
    ) -> Result<Self, tokio_postgres::Error> {
        self.1 = Some(client.prepare(self.0).await?);
        Ok(self)
    }
    pub fn bind<
        'c,
        'a,
        's,
        C: GenericClient,
        T1: crate::StringSql,
        T2: crate::StringSql,
        T3: crate::StringSql,
        T4: crate::StringSql,
    >(
        &'s self,
        client: &'c C,
        query: &'a T1,
        generation: &'a T2,
        provider: &'a Option<T3>,
        cutoff: &'a Option<i64>,
        exclude_node: &'a Option<T4>,
        limit: &'a i64,
    ) -> SearchQuery<'c, 'a, 's, C, Search, 6> {
        SearchQuery {
            client,
            params: [query, generation, provider, cutoff, exclude_node, limit],
            query: self.0,
            cached: self.1.as_ref(),
            extractor:
                |row: &tokio_postgres::Row| -> Result<SearchBorrowed, tokio_postgres::Error> {
                    Ok(SearchBorrowed {
                        node_id: row.try_get(0)?,
                        id: row.try_get(1)?,
                        conversation_id: row.try_get(2)?,
                        provider: row.try_get(3)?,
                        role: row.try_get(4)?,
                        projection: row.try_get(5)?,
                        score: row.try_get(6)?,
                        created_at: row.try_get(7)?,
                        title: row.try_get(8)?,
                        project: row.try_get(9)?,
                        repository: row.try_get(10)?,
                        conversation_created_at: row.try_get(11)?,
                        conversation_updated_at: row.try_get(12)?,
                    })
                },
            mapper: |it| Search::from(it),
        }
    }
}
impl<
    'c,
    'a,
    's,
    C: GenericClient,
    T1: crate::StringSql,
    T2: crate::StringSql,
    T3: crate::StringSql,
    T4: crate::StringSql,
>
    crate::client::async_::Params<
        'c,
        'a,
        's,
        SearchParams<T1, T2, T3, T4>,
        SearchQuery<'c, 'a, 's, C, Search, 6>,
        C,
    > for SearchStmt
{
    fn params(
        &'s self,
        client: &'c C,
        params: &'a SearchParams<T1, T2, T3, T4>,
    ) -> SearchQuery<'c, 'a, 's, C, Search, 6> {
        self.bind(
            client,
            &params.query,
            &params.generation,
            &params.provider,
            &params.cutoff,
            &params.exclude_node,
            &params.limit,
        )
    }
}
pub struct SemanticStmt(&'static str, Option<tokio_postgres::Statement>);
pub fn semantic() -> SemanticStmt {
    SemanticStmt(
        "SELECT m.node_id,m.id,m.conversation_id,c.provider,m.role,m.projection, 1.0-(m.vector <=> $1::text::public.vector) AS score, m.created_at,c.title,c.project,c.repository,c.created_at AS conversation_created_at,c.updated_at AS conversation_updated_at FROM messages m JOIN conversations c ON c.node_id=m.node_id AND c.id=m.conversation_id WHERE generation=$2 AND vector_dims(vector)=$3 AND projection<>'' AND ($4::text IS NULL OR c.provider=$4) AND ($5::bigint IS NULL OR m.created_at>=$5) AND ($6::text IS NULL OR m.node_id<>$6) ORDER BY score DESC,m.node_id,m.id LIMIT $7",
        None,
    )
}
impl SemanticStmt {
    pub async fn prepare<'a, C: GenericClient>(
        mut self,
        client: &'a C,
    ) -> Result<Self, tokio_postgres::Error> {
        self.1 = Some(client.prepare(self.0).await?);
        Ok(self)
    }
    pub fn bind<
        'c,
        'a,
        's,
        C: GenericClient,
        T1: crate::StringSql,
        T2: crate::StringSql,
        T3: crate::StringSql,
        T4: crate::StringSql,
    >(
        &'s self,
        client: &'c C,
        vector: &'a T1,
        generation: &'a T2,
        dimensions: &'a i32,
        provider: &'a Option<T3>,
        cutoff: &'a Option<i64>,
        exclude_node: &'a Option<T4>,
        limit: &'a i64,
    ) -> SearchQuery<'c, 'a, 's, C, Search, 7> {
        SearchQuery {
            client,
            params: [
                vector,
                generation,
                dimensions,
                provider,
                cutoff,
                exclude_node,
                limit,
            ],
            query: self.0,
            cached: self.1.as_ref(),
            extractor:
                |row: &tokio_postgres::Row| -> Result<SearchBorrowed, tokio_postgres::Error> {
                    Ok(SearchBorrowed {
                        node_id: row.try_get(0)?,
                        id: row.try_get(1)?,
                        conversation_id: row.try_get(2)?,
                        provider: row.try_get(3)?,
                        role: row.try_get(4)?,
                        projection: row.try_get(5)?,
                        score: row.try_get(6)?,
                        created_at: row.try_get(7)?,
                        title: row.try_get(8)?,
                        project: row.try_get(9)?,
                        repository: row.try_get(10)?,
                        conversation_created_at: row.try_get(11)?,
                        conversation_updated_at: row.try_get(12)?,
                    })
                },
            mapper: |it| Search::from(it),
        }
    }
}
impl<
    'c,
    'a,
    's,
    C: GenericClient,
    T1: crate::StringSql,
    T2: crate::StringSql,
    T3: crate::StringSql,
    T4: crate::StringSql,
>
    crate::client::async_::Params<
        'c,
        'a,
        's,
        SemanticParams<T1, T2, T3, T4>,
        SearchQuery<'c, 'a, 's, C, Search, 7>,
        C,
    > for SemanticStmt
{
    fn params(
        &'s self,
        client: &'c C,
        params: &'a SemanticParams<T1, T2, T3, T4>,
    ) -> SearchQuery<'c, 'a, 's, C, Search, 7> {
        self.bind(
            client,
            &params.vector,
            &params.generation,
            &params.dimensions,
            &params.provider,
            &params.cutoff,
            &params.exclude_node,
            &params.limit,
        )
    }
}
