//! Awaited protobuf mutations, without the SDK's automatic operation retry layer.
use super::{AppError, Condition, Filter, Point, endpoint, failure};
use prost::Message;
use qdrant_client::{Payload, qdrant as q};
use serde::Serialize;
use std::{collections::HashMap, time::Duration};
use tonic::{
    Request,
    metadata::{Ascii, MetadataValue},
    transport::{Channel, ClientTlsConfig, Endpoint},
};

pub(super) const MAX_REQUEST_BYTES: usize = 30 * 1024 * 1024;
const WRITE_TIMEOUT: Duration = Duration::from_mins(30);

#[derive(Clone)]
pub(super) struct Client {
    points: q::points_client::PointsClient<Channel>,
    key: Option<MetadataValue<Ascii>>,
}

impl Client {
    pub(super) fn new(url: &str, key: Option<&str>) -> Result<Self, AppError> {
        let url = endpoint(url)?;
        let mut channel = Endpoint::from_shared(url.to_string())
            .map_err(|_| failure())?
            .connect_timeout(Duration::from_secs(5))
            .timeout(WRITE_TIMEOUT);
        if url.scheme() == "https" {
            channel = channel
                .tls_config(ClientTlsConfig::new().with_native_roots())
                .map_err(|_| failure())?;
        }
        let key = key
            .map(|key| {
                if key.is_empty() {
                    return Err(AppError::configuration("Qdrant API key must be nonempty"));
                }
                let mut value: MetadataValue<Ascii> = key
                    .parse()
                    .map_err(|_| AppError::configuration("invalid Qdrant API key"))?;
                value.set_sensitive(true);
                Ok(value)
            })
            .transpose()?;
        Ok(Self {
            points: q::points_client::PointsClient::new(channel.connect_lazy())
                .max_encoding_message_size(MAX_REQUEST_BYTES)
                .max_decoding_message_size(1024 * 1024),
            key,
        })
    }

    fn request<T: Message>(&self, input: T) -> Result<Request<T>, AppError> {
        if input.encoded_len() > MAX_REQUEST_BYTES {
            return Err(AppError::usage(
                "Qdrant mutation exceeds the request-size limit",
            ));
        }
        let mut request = Request::new(input);
        request.set_timeout(WRITE_TIMEOUT);
        if let Some(key) = &self.key {
            request.metadata_mut().insert("api-key", key.clone());
        }
        Ok(request)
    }

    pub(super) async fn upsert(
        &self,
        name: &str,
        points: Vec<q::PointStruct>,
        wait: bool,
    ) -> Result<(), AppError> {
        let request = self.request(q::UpsertPoints {
            collection_name: name.into(),
            points,
            wait: Some(wait),
            ordering: Some(ordering()),
            timeout: Some(WRITE_TIMEOUT.as_secs()),
            ..Default::default()
        })?;
        let response = self
            .points
            .clone()
            .upsert(request)
            .await
            .map_err(|_| failure())?
            .into_inner();
        if wait {
            completed(&response)
        } else {
            accepted(&response)
        }
    }

    pub(super) async fn delete(
        &self,
        name: &str,
        filter: Filter,
        wait: bool,
    ) -> Result<(), AppError> {
        let request = self.request(q::DeletePoints {
            collection_name: name.into(),
            wait: Some(wait),
            ordering: Some(ordering()),
            points: Some(q::PointsSelector {
                points_selector_one_of: Some(q::points_selector::PointsSelectorOneOf::Filter(
                    filter.into(),
                )),
            }),
            timeout: Some(WRITE_TIMEOUT.as_secs()),
            ..Default::default()
        })?;
        let response = self
            .points
            .clone()
            .delete(request)
            .await
            .map_err(|_| failure())?
            .into_inner();
        if wait {
            completed(&response)
        } else {
            accepted(&response)
        }
    }
}

fn ordering() -> q::WriteOrdering {
    q::WriteOrdering {
        r#type: q::WriteOrderingType::Strong as i32,
    }
}

fn completed(response: &q::PointsOperationResponse) -> Result<(), AppError> {
    if response
        .result
        .as_ref()
        .is_some_and(|r| r.status == q::UpdateStatus::Completed as i32)
    {
        Ok(())
    } else {
        Err(failure())
    }
}

fn accepted(response: &q::PointsOperationResponse) -> Result<(), AppError> {
    if response.result.as_ref().is_some_and(|r| {
        r.status == q::UpdateStatus::Acknowledged as i32
            || r.status == q::UpdateStatus::Completed as i32
    }) {
        Ok(())
    } else {
        Err(failure())
    }
}

pub(super) fn point<P: Serialize>(point: &Point<P>) -> Result<q::PointStruct, AppError> {
    let mut vectors = HashMap::<String, q::Vector>::new();
    if let Some((name, values)) = &point.vector.dense {
        #[allow(clippy::cast_possible_truncation)]
        let values: Vec<f32> = values.iter().map(|v| *v as f32).collect();
        if values.iter().any(|v| !v.is_finite()) {
            return Err(failure());
        }
        vectors.insert(name.clone(), values.into());
    }
    // Generic JSON is confined to this SDK boundary; persisted payload models
    // remain the common typed document and readiness contracts.
    let payload = Payload::try_from(serde_json::to_value(&point.payload).map_err(|_| failure())?)
        .map_err(|_| failure())?;
    Ok(q::PointStruct::new(point.id.clone(), vectors, payload))
}

impl From<Filter> for q::Filter {
    fn from(value: Filter) -> Self {
        Self {
            must: value.must.into_iter().map(Into::into).collect(),
            should: value.should.into_iter().map(Into::into).collect(),
            must_not: value.must_not.into_iter().map(Into::into).collect(),
            ..Default::default()
        }
    }
}

impl From<Condition> for q::Condition {
    fn from(value: Condition) -> Self {
        use q::condition::ConditionOneOf as C;
        let condition_one_of = Some(match value {
            Condition::Nested(filter) => C::Filter(filter.into()),
            Condition::Match { key, r#match } => C::Field(q::FieldCondition {
                key,
                r#match: Some(q::Match {
                    match_value: Some(q::r#match::MatchValue::Keywords(q::RepeatedStrings {
                        strings: r#match.any,
                    })),
                }),
                ..Default::default()
            }),
            Condition::Text { key, r#match } => C::Field(q::FieldCondition {
                key,
                r#match: Some(q::Match {
                    match_value: Some(q::r#match::MatchValue::TextAny(r#match.text_any)),
                }),
                ..Default::default()
            }),
            Condition::Range { key, range } => {
                #[allow(clippy::cast_precision_loss)]
                let range = q::Range {
                    gte: range.gte.map(|v| v as f64),
                    lt: range.lt.map(|v| v as f64),
                    ..Default::default()
                };
                C::Field(q::FieldCondition {
                    key,
                    range: Some(range),
                    ..Default::default()
                })
            }
        });
        Self { condition_one_of }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_completed_mutations_are_acknowledged() {
        assert!(completed(&q::PointsOperationResponse::default()).is_err());
        for status in [0, 1, 2, 3, 4, 99] {
            let response = q::PointsOperationResponse {
                result: Some(q::UpdateResult {
                    status,
                    ..Default::default()
                }),
                ..Default::default()
            };
            assert_eq!(
                completed(&response).is_ok(),
                status == q::UpdateStatus::Completed as i32
            );
        }
    }

    #[tokio::test]
    async fn requests_bound_bytes_and_attach_sensitive_credentials_and_deadlines() {
        let client = Client::new("http://127.0.0.1:9", Some("private-key")).unwrap();
        let request = client.request(q::UpsertPoints::default()).unwrap();
        assert!(request.metadata().get("api-key").unwrap().is_sensitive());
        assert_eq!(request.metadata().get("api-key").unwrap(), "private-key");
        assert!(request.metadata().contains_key("grpc-timeout"));
        let oversized = q::UpsertPoints {
            collection_name: "x".repeat(MAX_REQUEST_BYTES),
            ..Default::default()
        };
        assert!(client.request(oversized).is_err());
        for key in ["", "private\nkey"] {
            let error = Client::new("http://127.0.0.1:9", Some(key)).err().unwrap();
            assert!(!error.error.message.contains("private"));
        }
    }

    #[test]
    fn protobuf_points_keep_named_vectors_and_typed_payloads() {
        let point = point(&Point {
            id: "00000000-0000-0000-0000-000000000001".into(),
            vector: super::super::Vectors {
                dense: Some(("dense_2".into(), vec![0.5, -0.25])),
            },
            payload: serde_json::json!({"text":"λ\nquoted", "count":42}),
        })
        .unwrap();
        assert_eq!(
            point.payload["text"].as_str().map(String::as_str),
            Some("λ\nquoted")
        );
        assert_eq!(point.payload["count"].as_integer(), Some(42));
        let Some(q::vectors::VectorsOptions::Vectors(vectors)) =
            point.vectors.unwrap().vectors_options
        else {
            panic!("named vectors")
        };
        // Word search ranks from the payload text index, not a vector.
        assert_eq!(vectors.vectors.len(), 1);
        assert!(vectors.vectors.contains_key("dense_2"));
    }

    #[test]
    fn delete_filters_preserve_node_and_exclusion_scopes() {
        let filter: q::Filter = Filter {
            must: vec![super::super::matches("node_id", vec!["node".into()])],
            must_not: vec![Condition::Nested(Filter {
                should: vec![super::super::matches("unit.anchor", vec!["keep".into()])],
                ..Default::default()
            })],
            ..Default::default()
        }
        .into();
        assert_eq!(filter.must.len(), 1);
        assert_eq!(filter.must_not.len(), 1);
        let Some(q::condition::ConditionOneOf::Filter(nested)) =
            &filter.must_not[0].condition_one_of
        else {
            panic!("nested exclusion")
        };
        let Some(q::condition::ConditionOneOf::Field(field)) = &nested.should[0].condition_one_of
        else {
            panic!("retained anchor")
        };
        assert_eq!(field.key, "unit.anchor");
        assert_eq!(
            field.r#match.as_ref().unwrap().match_value,
            Some(q::r#match::MatchValue::Keywords(q::RepeatedStrings {
                strings: vec!["keep".into()]
            }))
        );
    }
}
