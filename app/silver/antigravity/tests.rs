use super::*;
use crate::{
    silver::{IndexOptions, index},
    storage::Storage,
};
pub(crate) use mash_silver::antigravity::fixture::*;
use prost::Message;
use rusqlite::{Connection, params};

mod app;
mod bounds;
mod history;
mod mutations;
mod retrieval;

fn source(path: &Path) -> SourceIdentity {
    discover(path).unwrap().remove(0).identity
}
fn chat(path: &Path) -> Chat {
    parse(&source(path)).unwrap().chat.unwrap()
}
fn options(path: &Path) -> IndexOptions {
    IndexOptions {
        providers: [(Provider::Antigravity, vec![path.into()])].into(),
        roots_are_authoritative: true,
        since_days: None,
    }
}

fn replace_step(path: &Path, index: i64, step: &proto::Step) {
    Connection::open(path)
        .unwrap()
        .execute(
            "UPDATE steps SET step_type=?1,status=?2,metadata=?3,step_payload=?4 WHERE idx=?5",
            params![
                step.kind,
                step.status,
                step.metadata.as_ref().map(Message::encode_to_vec),
                step.encode_to_vec(),
                index
            ],
        )
        .unwrap();
}
