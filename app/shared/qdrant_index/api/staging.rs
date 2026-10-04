//! Bound staged writes by their actual encoded size, retaining completed-write semantics.
use super::{Client, Operation, OperationStatus, Point, UpsertSlice, failure};
use crate::AppError;
use reqwest::Method;
use serde::Serialize;
use serde_json::value::{RawValue, to_raw_value};

const MAX_POINTS: usize = 512;
const TARGET_BYTES: usize = 2 * 1024 * 1024;
const MAX_POINT_BYTES: usize = 30 * 1024 * 1024 - 1024;

#[derive(Default)]
pub(in crate::shared::qdrant_index) struct StagingBatch {
    points: Vec<Box<RawValue>>,
    bytes: usize,
}

impl StagingBatch {
    pub(in crate::shared::qdrant_index) async fn push<P: Serialize>(
        &mut self,
        client: &Client,
        collection: &str,
        point: Point<P>,
    ) -> Result<(), AppError> {
        // Serialize the typed record once. RawValue retains those validated bytes
        // for sizing and transport, without an untyped mapping or a second codec.
        let point = to_raw_value(&point).map_err(|_| failure())?;
        let bytes = point.get().len();
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

    pub(in crate::shared::qdrant_index) async fn flush(
        &mut self,
        client: &Client,
        collection: &str,
    ) -> Result<(), AppError> {
        if self.points.is_empty() {
            return Ok(());
        }
        let input = UpsertSlice {
            points: &self.points,
            update_filter: None,
            update_mode: None,
        };
        let result: Operation = client
            .call(
                Method::PUT,
                &format!("collections/{collection}/points?wait=true&ordering=strong"),
                Some(&input),
            )
            .await?;
        if result.status != OperationStatus::Completed {
            return Err(failure());
        }
        self.points.clear();
        self.bytes = 0;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
