//! Bound staged writes by their actual encoded size, retaining completed-write semantics.
use super::{Client, Point, grpc};
use crate::AppError;
use prost::Message;
use serde::Serialize;

const MAX_POINTS: usize = 512;
const TARGET_BYTES: usize = 2 * 1024 * 1024;
const MAX_POINT_BYTES: usize = 30 * 1024 * 1024 - 1024;

#[derive(Default)]
pub(in crate::views::qdrant) struct StagingBatch {
    points: Vec<qdrant_client::qdrant::PointStruct>,
    bytes: usize,
}

impl StagingBatch {
    pub(in crate::views::qdrant) async fn push<P: Serialize>(
        &mut self,
        client: &Client,
        collection: &str,
        point: Point<P>,
    ) -> Result<(), AppError> {
        let point = grpc::point(&point)?;
        let encoded = point.encoded_len();
        // Each repeated point includes its protobuf field tag and length prefix.
        let bytes = 1 + prost::length_delimiter_len(encoded) + encoded;
        if bytes > MAX_POINT_BYTES {
            return Err(AppError::usage(
                "Qdrant document exceeds the request-size limit",
            ));
        }
        if !self.points.is_empty()
            && (self.points.len() == MAX_POINTS || self.bytes + bytes + 1 > TARGET_BYTES)
        {
            self.flush(client, collection).await?;
        }
        self.bytes += bytes + 1;
        self.points.push(point);
        // A single large point may exceed the target, but remains below the
        // transport's hard limit. Send it alone rather than accumulating more.
        if self.bytes >= TARGET_BYTES || self.points.len() == MAX_POINTS {
            self.flush(client, collection).await?;
        }
        Ok(())
    }

    #[tracing::instrument(name = "qdrant_staging_flush", skip_all, fields(points = self.points.len(), bytes = self.bytes))]
    pub(in crate::views::qdrant) async fn flush(
        &mut self,
        client: &Client,
        collection: &str,
    ) -> Result<(), AppError> {
        if self.points.is_empty() {
            return Ok(());
        }
        client
            .mutations()?
            .upsert(collection, self.points.clone())
            .await?;
        self.points.clear();
        self.bytes = 0;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
