//! Recorded current-turn provenance and timestamps, separate from model input.
use super::{invalid, proto, store::Store, text};
use crate::{AppError, role::Role};
use std::collections::{HashMap, VecDeque};

#[derive(Clone, Copy)]
pub struct Evidence {
    pub visible: bool,
    pub timestamp: Option<i64>,
}

pub type Recorded = HashMap<String, VecDeque<Evidence>>;

pub fn key(role: &Role, text: &str) -> String {
    crate::parsers::stable_id("cursor-evidence", &[role.as_str(), text])
}

pub fn timestamp(value: Option<u64>) -> Result<Option<i64>, AppError> {
    value
        .map(|t| {
            i64::try_from(t).map_err(|_| invalid("native timestamp exceeds milliseconds range"))
        })
        .transpose()
}

pub fn current(store: &mut Store, root: &proto::Root) -> Result<Recorded, AppError> {
    let mut recorded = Recorded::new();
    for id in &root.turns {
        let turn: proto::Turn = store.protobuf(id, &[1])?;
        if let Some(proto::TurnKind::Agent(turn)) = turn.kind {
            let user: proto::User = store.protobuf(&turn.user, &[])?;
            let content = if let Some(id) = user.text_blob {
                let external = String::from_utf8(store.blob(&id)?)
                    .map_err(|_| invalid("invalid external user text"))?;
                if !user.text.is_empty() && user.text != external {
                    return Err(invalid("external and inline user text disagree"));
                }
                external
            } else {
                user.text
            };
            let visible = user.simulated != Some(true)
                && user.simulated_reason.is_none_or(|r| r == 0)
                && user.sent_by_agent.is_none();
            push(
                &mut recorded,
                &Role::User,
                &content,
                Evidence {
                    visible,
                    timestamp: timestamp(user.started_at.or(user.completed_at))?,
                },
            );
            for id in &turn.steps {
                let step: proto::Step = store.protobuf(id, &[1])?;
                if let Some(proto::StepKind::Assistant(assistant)) = step.kind {
                    push(
                        &mut recorded,
                        &Role::Assistant,
                        &assistant.text,
                        Evidence {
                            visible: true,
                            timestamp: timestamp(assistant.started_at.or(assistant.completed_at))?,
                        },
                    );
                }
            }
        }
    }
    Ok(recorded)
}

fn push(recorded: &mut Recorded, role: &Role, content: &str, evidence: Evidence) {
    let projection = text::projection(role, content);
    recorded
        .entry(key(role, &projection))
        .or_default()
        .push_back(evidence);
}
