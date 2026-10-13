//! Native displayed prose is independent of attachments, reasoning and copies.
use super::{invalid, proto, store::Root, timestamp};
use crate::{
    AppError,
    message_context::MessageContext,
    provider::Provider,
    role::Role,
    silver::dialogue,
    silver::{NormalizedMessage, parsers},
};

pub(super) fn normalize(
    root: &Root,
    metadata: &proto::Metadata,
    index: i64,
    step: &proto::Step,
) -> Result<Option<NormalizedMessage>, AppError> {
    let kind = proto::Kind::try_from(step.kind);
    if kind == Ok(proto::Kind::User) && step.user.is_none()
        || kind == Ok(proto::Kind::Assistant) && step.assistant.is_none()
    {
        return Err(invalid("missing native dialogue payload"));
    }
    if step.user.is_some() && (step.assistant.is_some() || kind != Ok(proto::Kind::User))
        || step.assistant.is_some() && kind != Ok(proto::Kind::Assistant)
    {
        return Err(invalid("native step type and dialogue payload disagree"));
    }
    let (role, prose, content) = if let Some(user) = &step.user {
        let prose = if user.response.is_empty() {
            let fallback = user
                .items
                .iter()
                .map(|item| item.text.as_str())
                .collect::<String>();
            if fallback.is_empty() {
                user.legacy_query.clone()
            } else {
                fallback
            }
        } else {
            user.response.clone()
        };
        let mut content = prose.clone();
        for block in user
            .items
            .iter()
            .filter_map(|i| i.scope.as_ref()?.text_block.as_ref())
        {
            if !block.content.is_empty() {
                content.push('\n');
                content.push_str(&block.label);
                content.push('\n');
                content.push_str(&block.content);
            }
        }
        (Role::User, prose, content)
    } else if let Some(assistant) = &step.assistant {
        let content = [
            &assistant.response,
            &assistant.modified_response,
            &assistant.thinking,
            &assistant.raw_thinking,
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n");
        (Role::Assistant, assistant.response.clone(), content)
    } else {
        return Ok(None);
    };
    if content.trim().is_empty() {
        return Ok(None);
    }
    let origin = step.metadata.as_ref();
    let copied = origin.and_then(|m| m.origin.as_ref()).is_some_and(|r| {
        r.trajectory != root.trajectory || r.cascade != root.cascade || i64::from(r.step) != index
    });
    let visible = proto::TrajectoryKind::try_from(root.kind) == Ok(proto::TrajectoryKind::Cascade)
        && proto::TrajectorySource::try_from(root.source)
            == Ok(proto::TrajectorySource::CascadeClient)
        && metadata.subagent.is_none()
        && !copied
        && proto::Status::try_from(step.status) == Ok(proto::Status::Done)
        && match role {
            Role::User => origin.is_some_and(|m| {
                proto::Source::try_from(m.source) == Ok(proto::Source::UserExplicit)
            }),
            _ => origin
                .is_some_and(|m| proto::Source::try_from(m.source) == Ok(proto::Source::Model)),
        };
    let projection = if visible {
        dialogue::visible(&role, &prose).trim().to_owned()
    } else {
        String::new()
    };
    Ok(Some(NormalizedMessage {
        attachments: Vec::new(),
        id: parsers::stable_id(
            "message",
            &[
                Provider::Antigravity.as_str(),
                &root.cascade,
                &root.trajectory,
                &index.to_string(),
            ],
        ),
        ordinal: 0,
        context: MessageContext::Linear,
        role,
        content,
        dialogue_text: Some(projection),
        created_at: timestamp(origin.and_then(|m| m.created_at.as_ref()))?,
        tools: crate::tools::Activity::default(),
    }))
}
