//! HTTP transport fixture; no models or external services. Backend semantics
//! are verified separately against disposable Qdrant.
use axum::{
    Json, Router,
    routing::{get, post},
};
use serde_json::{Value, json};

pub(crate) struct Service {
    pub(crate) url: String,
    pub(crate) requests: tokio::sync::mpsc::UnboundedReceiver<Value>,
    task: tokio::task::JoinHandle<()>,
}
impl Service {
    pub(crate) async fn new() -> Self {
        Self::with_dense(true).await
    }
    pub(crate) async fn with_dense(dense: bool) -> Self {
        let (sender, requests) = tokio::sync::mpsc::unbounded_channel();
        let router = Router::new()
            .route(
                "/v1/capabilities",
                get(move || async move { Json(json!({"embedding_text":"dialogue", "representations":if dense { "lexical-and-dense" } else { "lexical-only" }})) }),
            )
            .route(
                "/embeddings",
                post(|| async {
                    Json(json!({"data":[{"index":0,"embedding":[1.0,0.0,0.0,0.0]}]}))
                }),
            )
            .route(
                "/v1/search",
                post(move |Json(body): Json<Value>| {
                    let sender = sender.clone();
                    async move {
                        sender.send(body).unwrap();
                        Json(json!({"lexical":[],"semantic":[],"documents":[]}))
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self {
            url,
            requests,
            task,
        }
    }
    pub(crate) fn configuration(&self) -> String {
        format!(
            "version=1\nproviders={{}}\n[gold.build]\nrepresentations='lexical-only'\n[gold.query]\nintent='exact-mention'\n[shared]\ngroup_id='transport'\nnode_id='node'\napi_url='{}'\ntoken_env='PATH'\n[embeddings]\nurl='{}/embeddings'\nmodel='test'\ndimensions=4\n",
            self.url, self.url
        )
    }
    pub(crate) async fn received(&mut self) -> Value {
        tokio::time::timeout(std::time::Duration::from_secs(5), self.requests.recv())
            .await
            .unwrap()
            .unwrap()
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.task.abort();
    }
}
