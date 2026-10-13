//! A completed context followed by a stalled response, with drop observation.
use super::{Message, protocol};
use axum::{Json, body::Body, extract::State};
use futures_util::{StreamExt, stream};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

struct Dropped(Arc<AtomicBool>);
impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

pub(super) async fn view(
    State(dropped): State<Arc<AtomicBool>>,
    Json(request): Json<protocol::Contexts>,
) -> Body {
    let (index, request) = request
        .anchors
        .into_iter()
        .enumerate()
        .find(|(_, anchor)| anchor.id == "fast")
        .unwrap();
    let messages = vec![
        Message {
            attachments: Vec::new(),
            id: request.id,
            ordinal: 0,
            role: Some(crate::role::Role::User),
            author_kind: None,
            content: "cache the model".into(),
            created_at: None,
            tools: crate::tools::Activity::default(),
        },
        Message {
            attachments: Vec::new(),
            id: "nearby".into(),
            ordinal: 1,
            role: Some(crate::role::Role::Assistant),
            author_kind: None,
            content: "keep the worker warm".into(),
            created_at: None,
            tools: crate::tools::Activity::default(),
        },
    ];
    let mut bytes =
        serde_json::to_vec(&protocol::ContextResult::Complete { index, messages }).unwrap();
    bytes.push(b'\n');
    Body::from_stream(
        stream::iter([Ok::<_, std::io::Error>(bytes)]).chain(stream::once(async move {
            let _guard = Dropped(dropped);
            std::future::pending::<Result<Vec<u8>, std::io::Error>>().await
        })),
    )
}
