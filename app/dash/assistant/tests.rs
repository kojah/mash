use super::{Call, CallDelta, Function, FunctionDelta, MAX_LOOKUPS, chunk, clip, gather};
use serde_json::{Value, json};
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

const TOKEN: &str = "0123456789abcdef0123456789abcdef";

/// What a stand-in model service answers one request with.
type Script = Arc<dyn Fn(&Value) -> (u16, String) + Send + Sync>;

struct Fixture {
    _directory: tempfile::TempDir,
    address: std::net::SocketAddr,
    /// Every request the stand-in service received, with its bearer token.
    asked: Arc<Mutex<Vec<(String, Value)>>>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

fn says(text: &[&str]) -> String {
    let mut body = String::from(": processing\n\n");
    for delta in text {
        let _ = write!(
            body,
            "data: {}\n\n",
            json!({"choices": [{"delta": {"content": delta}}]})
        );
    }
    body.push_str("data: [DONE]\n\n");
    body
}

/// A request for lookups, with each one's arguments split over two chunks as
/// services send them.
fn wants(calls: &[(&str, &str)]) -> String {
    let mut body = String::new();
    for (index, (name, arguments)) in calls.iter().enumerate() {
        let (head, tail) = arguments.split_at(arguments.len() / 2);
        let first = json!({"choices": [{"delta": {"tool_calls": [{"index": index, "id": format!("call-{index}"), "type": "function", "function": {"name": name, "arguments": head}}]}}]});
        let second = json!({"choices": [{"delta": {"tool_calls": [{"index": index, "function": {"arguments": tail}}]}}]});
        let _ = write!(body, "data: {first}\n\ndata: {second}\n\n");
    }
    body.push_str("data: [DONE]\n\n");
    body
}

async fn fixture(assistant: Option<Script>, access: Option<&str>) -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("index.sqlite3");
    let mut storage = crate::storage::Storage::open_writer(&database).unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    let connection = rusqlite::Connection::open(&database).unwrap();
    crate::storage::packed::register(&connection).unwrap();
    connection.execute_batch("INSERT INTO chats(id,provider,title) VALUES('busy','codex','Port decision'); INSERT INTO messages(id,chat_id,ordinal,role,content_packed) VALUES('first','busy',0,'user','Which port does the fixture use?'),('second','busy',1,'assistant','It uses port 8080.'),('third','busy',2,'tool','raw tool output');").unwrap();
    drop(connection);
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut tasks = Vec::new();
    let mut config = String::from("version = 1\nproviders = {}\n");
    if let Some(script) = assistant {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        // Any variable that is always set stands in for the key.
        let _ = write!(
            config,
            "[assistant]\nbase_url = 'http://{}/api/v1/'\ntoken_env = 'PATH'\nmodel = 'test/model'\n",
            listener.local_addr().unwrap()
        );
        let record = asked.clone();
        let service = axum::Router::new().route(
            "/api/v1/chat/completions",
            axum::routing::post(
                move |headers: axum::http::HeaderMap, axum::Json(body): axum::Json<Value>| {
                    let (status, text) = script(&body);
                    let token = headers["authorization"].to_str().unwrap().to_owned();
                    record.lock().unwrap().push((token, body));
                    async move {
                        (
                            axum::http::StatusCode::from_u16(status).unwrap(),
                            [("content-type", "text/event-stream")],
                            text,
                        )
                    }
                },
            ),
        );
        tasks.push(tokio::spawn(async move {
            axum::serve(listener, service).await.unwrap();
        }));
    }
    let path = directory.path().join("config.toml");
    std::fs::write(&path, config).unwrap();
    let paths = crate::core::Paths {
        config: Some(path),
        db: Some(database),
        models_dir: None,
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = super::super::router(Arc::new(super::Dash {
        console: crate::mcp::console::Console::new(paths.clone()),
        paths,
        hosts: super::super::allowed_hosts(address),
        loopback: access.is_none(),
        access: super::super::access::Access::new(access),
        shared_only: false,
        model_work: tokio::sync::Semaphore::new(0),
        services: super::super::services::Services::load(None).unwrap(),
        live: super::super::live::Live::new(None, None),
        questions: Arc::new(tokio::sync::Semaphore::new(super::MAX_QUESTIONS)),
    }));
    tasks.push(tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    }));
    Fixture {
        _directory: directory,
        address,
        asked,
        tasks,
    }
}

impl Fixture {
    fn url(&self) -> String {
        format!("http://{}/api/assistant", self.address)
    }

    /// Asks a question and returns the events of the answer.
    async fn ask(&self, body: Value) -> Vec<Value> {
        let response = reqwest::Client::new()
            .post(self.url())
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "text/event-stream");
        response
            .text()
            .await
            .unwrap()
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(|data| serde_json::from_str(data.trim()).unwrap())
            .collect()
    }
}

fn question(text: &str) -> Value {
    json!({"messages": [{"role": "user", "content": text}]})
}

#[tokio::test]
async fn a_question_is_answered_from_lookups_the_model_asks_for_and_nothing_else_runs() {
    let script: Script = Arc::new(|body| {
        let asked_already = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["role"] == "tool");
        if asked_already {
            (200, says(&["\n\n", "It uses ", "port 8080."]))
        } else {
            (
                200,
                format!(
                    "data: {}\n\n{}",
                    json!({"choices": [{"delta": {"content": "\n\n"}}]}),
                    wants(&[
                        ("read_chat", r#"{"chat_id":"busy"}"#),
                        ("forget", r#"{"chat_id":"busy"}"#),
                        ("recent_chats", "not json"),
                    ])
                ),
            )
        }
    });
    let fixture = fixture(Some(script), None).await;
    let mut body = question("which port does the fixture use?");
    body["open_chat"] = json!({"id": "busy", "title": "Port decision"});
    let events = fixture.ask(body).await;
    assert_eq!(
        events,
        [
            json!({"type": "searching"}),
            json!({"type": "text", "delta": "It uses "}),
            json!({"type": "text", "delta": "port 8080."}),
            json!({"type": "done", "partial": false}),
        ]
    );
    let asked = fixture.asked.lock().unwrap().clone();
    let (token, first) = &asked[0];
    // The key goes to the model's service and the conversation says which chat is open.
    assert_eq!(token, &format!("Bearer {}", std::env::var("PATH").unwrap()));
    assert_eq!(first["model"], "test/model");
    assert_eq!(first["stream"], true);
    assert_eq!(first["tool_choice"], "auto");
    // No schema uses references or alternatives, which some services refuse.
    let schemas = first["tools"].to_string();
    assert!(!schemas.contains("$ref") && !schemas.contains("oneOf") && !schemas.contains("anyOf"));
    let offered: Vec<&str> = first["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        offered,
        [
            "search",
            "recent_chats",
            "read_chat",
            "read_message",
            "memories"
        ]
    );
    assert!(
        first["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("\"busy\"")
    );
    let results: Vec<&str> = asked[1].1["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "tool")
        .map(|message| message["content"].as_str().unwrap())
        .collect();
    // The chat is read as dialogue, without tool output.
    assert!(
        results[0].contains("assistant: It uses port 8080."),
        "{}",
        results[0]
    );
    assert!(!results[0].contains("raw tool output"));
    // A tool that changes data is not a lookup, whatever the model asks for.
    assert_eq!(results[1], "There is no such lookup.");
    assert_eq!(results[2], "The arguments do not fit this lookup.");
    // The chat is still there.
    let page: Value = reqwest::get(format!("http://{}/api/chat?id=busy", fixture.address))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(page["messages"].as_array().unwrap().len(), 3);
}

fn told_to_answer(body: &Value) -> bool {
    body["messages"]
        .as_array()
        .unwrap()
        .last()
        .is_some_and(|message| message["role"] == "user" && message["content"] == super::ANSWER_NOW)
}

#[tokio::test]
async fn lookups_run_out_and_the_model_is_told_to_answer_with_what_it_has() {
    let script: Script = Arc::new(|body| {
        if told_to_answer(body) {
            (200, says(&["Here is what I found."]))
        } else {
            (200, wants(&[("recent_chats", "{}"), ("memories", "{}")]))
        }
    });
    let fixture = fixture(Some(script), None).await;
    let events = fixture.ask(question("everything about everything")).await;
    assert_eq!(
        events[events.len() - 2..],
        [
            json!({"type": "text", "delta": "Here is what I found."}),
            json!({"type": "done", "partial": true}),
        ]
    );
    let asked = fixture.asked.lock().unwrap();
    // Two lookups a round until the limit, then one request for the answer.
    assert_eq!(asked.len(), MAX_LOOKUPS / 2 + 1);
    let lookups = asked.last().unwrap().1["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "tool")
        .count();
    assert_eq!(lookups, MAX_LOOKUPS);
}

#[tokio::test]
async fn a_model_that_never_answers_is_stopped_and_the_reader_is_told() {
    let script: Script = Arc::new(|_| (200, wants(&[("recent_chats", "{\"days\": 7}")])));
    let fixture = fixture(Some(script), None).await;
    let events = fixture.ask(question("anything")).await;
    assert_eq!(
        events[events.len() - 2..],
        [
            json!({"type": "text", "delta": super::NOTHING_TO_SAY}),
            json!({"type": "done", "partial": true}),
        ]
    );
    // Every lookup allowed, one more request, and nothing after that.
    assert_eq!(fixture.asked.lock().unwrap().len(), MAX_LOOKUPS + 1);
}

/// A reply the service ended at its length limit, as a model that spent the
/// limit thinking sends it: little or no text, then `length`.
fn runs_out(text: &str) -> String {
    let mut body = String::new();
    if !text.is_empty() {
        let _ = write!(
            body,
            "data: {}\n\n",
            json!({"choices": [{"delta": {"content": text}}]})
        );
    }
    let _ = write!(
        body,
        "data: {}\n\ndata: [DONE]\n\n",
        json!({"choices": [{"delta": {}, "finish_reason": "length"}], "usage": {"prompt_tokens": 9, "completion_tokens": 4000}})
    );
    body
}

#[tokio::test]
async fn a_reply_cut_off_at_its_length_limit_is_said_to_be_cut_off() {
    let nothing = fixture(Some(Arc::new(|_| (200, runs_out("")))), None).await;
    let events = nothing.ask(question("What did I do yesterday?")).await;
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0]["type"], "failed");
    assert_eq!(events[0]["reason"], "failed");
    assert!(
        events[0]["message"]
            .as_str()
            .unwrap()
            .contains("ran out of room")
    );
    assert_eq!(nothing.asked.lock().unwrap().len(), 1);

    let some = fixture(Some(Arc::new(|_| (200, runs_out("It uses port")))), None).await;
    let events = some.ask(question("Which port?")).await;
    assert_eq!(events[0], json!({"type": "text", "delta": "It uses port"}));
    assert_eq!(events[1], json!({"type": "done", "partial": true}));
    assert!(
        some.asked.lock().unwrap()[0].1["max_tokens"]
            .as_u64()
            .unwrap()
            >= 4000
    );
}

#[tokio::test]
async fn a_service_that_refuses_or_limits_is_reported_once_and_not_retried() {
    for (status, reason) in [
        (401, "refused"),
        (402, "refused"),
        (429, "limited"),
        (500, "failed"),
    ] {
        let script: Script =
            Arc::new(move |_| (status, "{\"error\":{\"message\":\"secret detail\"}}".into()));
        let fixture = fixture(Some(script), None).await;
        let events = fixture.ask(question("anything")).await;
        assert_eq!(events.len(), 1, "{status}");
        assert_eq!(events[0]["type"], "failed");
        assert_eq!(events[0]["reason"], reason, "{status}");
        assert!(!events[0].to_string().contains("secret detail"));
        assert_eq!(fixture.asked.lock().unwrap().len(), 1, "{status}");
    }
}

#[tokio::test]
async fn there_is_no_assistant_without_configuration() {
    let fixture = fixture(None, None).await;
    let status: Value = reqwest::get(fixture.url())
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status, json!({"enabled": false, "model": null}));
    let refused = reqwest::Client::new()
        .post(fixture.url())
        .json(&question("anything"))
        .send()
        .await
        .unwrap();
    assert_eq!(refused.status(), reqwest::StatusCode::CONFLICT);
}

#[tokio::test]
async fn the_assistant_is_behind_the_sign_in_and_takes_questions_from_its_own_page_only() {
    let script: Script = Arc::new(|_| (200, says(&["Hello."])));
    let fixture = fixture(Some(script), Some(TOKEN)).await;
    let client = reqwest::Client::new();
    let stranger = client.get(fixture.url()).send().await.unwrap();
    assert_eq!(stranger.status(), reqwest::StatusCode::UNAUTHORIZED);
    let stranger = client
        .post(fixture.url())
        .json(&question("hi"))
        .send()
        .await
        .unwrap();
    assert_eq!(stranger.status(), reqwest::StatusCode::UNAUTHORIZED);
    let status: Value = client
        .get(fixture.url())
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    // The page learns the model's name and never the key.
    assert_eq!(status, json!({"enabled": true, "model": "test/model"}));
    let elsewhere = client
        .post(fixture.url())
        .bearer_auth(TOKEN)
        .header("Origin", "https://attacker.invalid")
        .json(&question("hi"))
        .send()
        .await
        .unwrap();
    assert_eq!(elsewhere.status(), reqwest::StatusCode::FORBIDDEN);
    for body in [
        json!({"messages": []}),
        json!({"messages": [{"role": "assistant", "content": "hi"}]}),
        json!({"messages": [{"role": "system", "content": "hi"}]}),
    ] {
        let invalid = client
            .post(fixture.url())
            .bearer_auth(TOKEN)
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(invalid.status(), reqwest::StatusCode::BAD_REQUEST, "{body}");
    }
    assert!(fixture.asked.lock().unwrap().is_empty());
}

#[test]
fn a_lookup_request_is_put_together_from_its_fragments() {
    let mut calls: Vec<Call> = Vec::new();
    let part = |index, id: Option<&str>, name: Option<&str>, arguments: &str| CallDelta {
        index,
        id: id.map(str::to_owned),
        function: Some(FunctionDelta {
            name: name.map(str::to_owned),
            arguments: Some(arguments.to_owned()),
        }),
    };
    gather(
        &mut calls,
        part(0, Some("a"), Some("search"), "{\"query\":"),
    );
    gather(&mut calls, part(1, Some("b"), Some("view"), "{}"));
    gather(&mut calls, part(0, None, None, "\"port\"}"));
    // A flood of calls is not kept.
    gather(&mut calls, part(500, Some("z"), Some("search"), "{}"));
    assert_eq!(calls.len(), 2);
    let Function { name, arguments } = &calls[0].function;
    assert_eq!(
        (name.as_str(), arguments.as_str()),
        ("search", "{\"query\":\"port\"}")
    );
    assert_eq!(calls[1].id, "b");
}

#[test]
fn only_data_lines_of_the_stream_are_read() {
    assert!(chunk(b": OPENROUTER PROCESSING\n").is_none());
    assert!(chunk(b"data: [DONE]\n").is_none());
    assert!(chunk(b"\n").is_none());
    let text = chunk(b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n").unwrap();
    assert_eq!(text.choices[0].delta.content.as_deref(), Some("hi"));
    assert!(
        chunk(b"data: {\"error\":{\"message\":\"overloaded\"}}\n")
            .unwrap()
            .error
            .is_some()
    );
}

#[test]
fn long_text_is_cut_on_a_character_boundary() {
    assert_eq!(clip("héllo", 10), "héllo");
    assert_eq!(clip("héllo", 2), "hé…");
}
