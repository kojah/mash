//! Saved step positions establish identity; provenance controls search projection.
use super::{invalid, proto, timestamp};
use crate::{
    AppError,
    message_context::MessageContext,
    provider::Provider,
    role::Role,
    silver::dialogue,
    silver::{NormalizedMessage, parsers},
};
use proto::{StepKind, StepSource, TrajectoryKind, TrajectorySource};

pub(super) fn messages(
    root: &proto::Trajectory,
    session: &str,
) -> Result<Vec<NormalizedMessage>, AppError> {
    let main = matches!(
        TrajectoryKind::try_from(root.kind),
        Ok(TrajectoryKind::Cascade | TrajectoryKind::InteractiveCascade)
    ) && matches!(
        TrajectorySource::try_from(root.source),
        Ok(TrajectorySource::Unspecified | TrajectorySource::CascadeClient)
    );
    let mut messages = Vec::new();
    let mut retained = 0usize;
    for (position, step) in root.steps.iter().enumerate() {
        if let Some(mut message) = normalize(step, main, session, position)? {
            retained = retained
                .saturating_add(message.content.capacity())
                .saturating_add(message.dialogue_text.as_ref().map_or(0, String::capacity))
                .saturating_add(512);
            if retained > 32 * 1024 * 1024 {
                return Err(invalid("native history exceeds retained replay limit"));
            }
            message.ordinal =
                i64::try_from(messages.len()).map_err(|_| invalid("too many native steps"))?;
            messages.push(message);
        }
    }
    Ok(messages)
}

fn normalize(
    step: &proto::Step,
    main: bool,
    session: &str,
    position: usize,
) -> Result<Option<NormalizedMessage>, AppError> {
    let kind = StepKind::try_from(step.kind);
    if step.user.is_some() && (step.assistant.is_some() || kind != Ok(StepKind::UserInput))
        || step.assistant.is_some() && kind != Ok(StepKind::PlannerResponse)
    {
        return Err(invalid("native step type and payload disagree"));
    }
    let (role, content, prose) = if let Some(user) = &step.user {
        (
            Role::User,
            join(&[&user.query, &user.response]),
            user.query.as_str(),
        )
    } else if let Some(assistant) = &step.assistant {
        (
            Role::Assistant,
            join(&[
                &assistant.response,
                &assistant.modified_response,
                &assistant.thinking,
            ]),
            assistant.response.as_str(),
        )
    } else {
        return Ok(None);
    };
    if content.trim().is_empty() {
        return Ok(None);
    }
    let origin = step
        .metadata
        .as_ref()
        .map_or(Ok(StepSource::Unspecified), |m| {
            StepSource::try_from(m.source)
        });
    let copied = step.metadata.as_ref().is_some_and(|m| m.copied.is_some());
    let visible = main
        && !copied
        && proto::Status::try_from(step.status).is_ok()
        && match role {
            Role::User => matches!(
                origin,
                Ok(StepSource::Unspecified | StepSource::Manual | StepSource::UserExplicit)
            ),
            _ => matches!(
                origin,
                Ok(StepSource::Unspecified | StepSource::Model | StepSource::Manual)
            ),
        };
    let projection = if visible {
        dialogue::visible(&role, prose).trim().to_owned()
    } else {
        String::new()
    };
    Ok(Some(NormalizedMessage {
        attachments: Vec::new(),
        id: parsers::stable_id(
            "message",
            &[Provider::Windsurf.as_str(), session, &position.to_string()],
        ),
        ordinal: 0,
        context: MessageContext::Linear,
        role,
        content,
        dialogue_text: Some(projection),
        created_at: timestamp(step.metadata.as_ref().and_then(|m| m.created_at.as_ref()))?,
    }))
}

fn join(parts: &[&str]) -> String {
    parts
        .iter()
        .copied()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}
