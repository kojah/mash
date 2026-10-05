//! Bounded typed NDJSON context reads retain evidence received before timeout.
use super::{ADMISSION, Endpoint, protocol, request};
use crate::{AppError, config::SharedConfig, storage::Message};
use std::collections::BTreeSet;
use tracing::Instrument;

type Completed = Vec<((String, String), Result<Vec<Message>, AppError>)>;
const MAX_LINE: usize = 4 * 1024 * 1024;
const MAX_BODY: usize = 64 * 1024 * 1024;

pub(in crate::shared) async fn read(
    config: &SharedConfig,
    anchors: Vec<(String, String)>,
) -> (Completed, Option<String>) {
    if anchors.is_empty() {
        return (Vec::new(), None);
    }
    let span = tracing::info_span!(
        "shared_http_request",
        endpoint = "contexts",
        trace_id = tracing::field::Empty,
        status = tracing::field::Empty,
        elapsed_ms = tracing::field::Empty,
        outcome = tracing::field::Empty,
    );
    let mut completed = Vec::new();
    let started = std::time::Instant::now();
    let result = tokio::time::timeout(
        super::super::retrieval::CONTEXT_DEADLINE,
        async {
            let _permit = Endpoint::Contexts
                .capacity(&ADMISSION)
                .acquire()
                .await
                .map_err(|_| invalid())?;
            let mut request = request(config, reqwest::Method::POST, Endpoint::Contexts)?;
            if !span.is_disabled()
                && let Some(id) = crate::observability::http::TraceId::generate()
            {
                span.record("trace_id", id.as_str());
                request = request.header(crate::observability::http::HEADER, id.as_str());
            }
            let input = protocol::Contexts {
                anchors: anchors
                    .iter()
                    .map(|(node, id)| protocol::View {
                        node_id: node.clone(),
                        id: id.clone(),
                        context: 2,
                        content: crate::storage::MessageContent::SearchProjection,
                    })
                    .collect(),
            };
            let mut response = request.json(&input).send().await.map_err(|_| {
                AppError::shared("shared context HTTP request failed; check API and network")
            })?;
            span.record("status", response.status().as_u16());
            if !response.status().is_success() {
                return Err(AppError::shared("shared context request was rejected"));
            }
            let mut decoder = Decoder::default();
            let mut seen = BTreeSet::new();
            while let Some(bytes) = response.chunk().await.map_err(|_| invalid())? {
                let (next_decoder, items) = crate::blocking(move || {
                    let items = decoder.push(&bytes)?;
                    Ok((decoder, items))
                })
                .await?;
                decoder = next_decoder;
                for item in items {
                    let index = item.index();
                    let key = anchors.get(index).ok_or_else(invalid)?;
                    if !seen.insert(index) {
                        return Err(invalid());
                    }
                    let result = match item {
                        protocol::ContextResult::Complete { messages, .. } => Ok(messages),
                        protocol::ContextResult::Failed { error, .. } => Err(error),
                    };
                    completed.push((key.clone(), result));
                }
            }
            if !decoder.pending.is_empty() || seen.len() != anchors.len() {
                return Err(invalid());
            }
            Ok(())
        }
        .instrument(span.clone()),
    )
    .await;
    let error = match result {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error.error.message),
        Err(_) => Some(
            "shared context deadline exceeded; ranking uses available message evidence".to_owned(),
        ),
    };
    span.record("elapsed_ms", crate::progress::milliseconds(started));
    span.record(
        "outcome",
        if error.is_none() {
            "success"
        } else {
            "partial"
        },
    );
    (completed, error)
}

fn invalid() -> AppError {
    AppError::shared("invalid shared context HTTP response")
}

#[derive(Default)]
struct Decoder {
    pending: Vec<u8>,
    bytes: usize,
}

impl Decoder {
    fn push(&mut self, bytes: &[u8]) -> Result<Vec<protocol::ContextResult>, AppError> {
        self.bytes = self.bytes.checked_add(bytes.len()).ok_or_else(invalid)?;
        if self.bytes > MAX_BODY {
            return Err(invalid());
        }
        let mut items = Vec::new();
        for piece in bytes.split_inclusive(|b| *b == b'\n') {
            if self.pending.len().saturating_add(piece.len()) > MAX_LINE {
                return Err(invalid());
            }
            self.pending.extend_from_slice(piece);
            if piece.last() == Some(&b'\n') {
                items.push(serde_json::from_slice(&self.pending).map_err(|_| invalid())?);
                self.pending.clear();
            }
        }
        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_survive_every_byte_boundary_and_out_of_order_indices() {
        let bytes = b"{\"status\":\"complete\",\"index\":1,\"messages\":[]}\n{\"status\":\"complete\",\"index\":0,\"messages\":[]}\n";
        for split in 0..=bytes.len() {
            let mut decoder = Decoder::default();
            let mut items = decoder.push(&bytes[..split]).unwrap();
            items.extend(decoder.push(&bytes[split..]).unwrap());
            assert_eq!(
                items
                    .iter()
                    .map(protocol::ContextResult::index)
                    .collect::<Vec<_>>(),
                [1, 0]
            );
            assert_eq!(decoder.pending, [] as [u8; 0]);
        }
    }

    #[test]
    fn malformed_unknown_and_oversized_frames_fail_closed() {
        for bytes in [
            b"{}\n".as_slice(),
            b"{\"status\":\"complete\",\"index\":0,\"messages\":[],\"extra\":true}\n",
            b"\n",
        ] {
            assert!(Decoder::default().push(bytes).is_err());
        }
        assert!(Decoder::default().push(&vec![b'x'; MAX_LINE + 1]).is_err());
        let mut decoder = Decoder {
            bytes: MAX_BODY,
            ..Decoder::default()
        };
        assert!(decoder.push(b"x").is_err());
    }
}
