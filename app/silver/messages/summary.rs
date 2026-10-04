use super::NormalizedMessage;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Summary {
    pub(crate) created_at: Option<i64>,
    pub(crate) updated_at: Option<i64>,
    pub(crate) created_message_id: Option<String>,
    pub(crate) updated_message_id: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) title_message_id: Option<String>,
    pub(crate) searchable: u64,
    pub(crate) searchable_users: u64,
}

impl Summary {
    pub(super) fn observe(&mut self, message: &NormalizedMessage) {
        if let Some(timestamp) = message.created_at {
            if self.created_at.is_none_or(|previous| timestamp < previous)
                || (self.created_at == Some(timestamp)
                    && self
                        .created_message_id
                        .as_ref()
                        .is_none_or(|id| message.id < *id))
            {
                self.created_message_id = Some(message.id.clone());
            }
            if self.updated_at.is_none_or(|previous| timestamp > previous)
                || (self.updated_at == Some(timestamp)
                    && self
                        .updated_message_id
                        .as_ref()
                        .is_none_or(|id| message.id < *id))
            {
                self.updated_message_id = Some(message.id.clone());
            }
            self.created_at = Some(
                self.created_at
                    .map_or(timestamp, |previous| previous.min(timestamp)),
            );
            self.updated_at = Some(
                self.updated_at
                    .map_or(timestamp, |previous| previous.max(timestamp)),
            );
        }
        let projection = message.dialogue_text.as_deref().unwrap_or(&message.content);
        self.searchable += u64::from(!projection.is_empty());
        self.searchable_users +=
            u64::from(!projection.is_empty() && message.role == crate::role::Role::User);
        if self.title.is_none() && message.role == crate::role::Role::User {
            self.title = crate::silver::dialogue::title(projection);
            if self.title.is_some() {
                self.title_message_id = Some(message.id.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundaries_include_hidden_messages_and_ties_match_inline_and_spooled_reads() {
        let mut rows = Vec::new();
        for (ordinal, (id, timestamp)) in [
            ("z", Some(10)),
            ("a", Some(10)),
            ("late", Some(20)),
            ("undated", None),
        ]
        .into_iter()
        .enumerate()
        {
            rows.push(NormalizedMessage {
                attachments: Vec::new(),
                id: id.into(),
                ordinal: i64::try_from(ordinal).unwrap(),
                role: crate::role::Role::User,
                context: crate::message_context::MessageContext::Linear,
                content: "hidden context".into(),
                dialogue_text: Some(String::new()),
                created_at: timestamp,
            });
        }
        let inline = super::super::Messages::Inline(rows.clone())
            .summary()
            .unwrap();
        let mut builder = super::super::Builder {
            limit: 0,
            ..Default::default()
        };
        for row in rows.into_iter().rev() {
            builder.push(row).unwrap();
        }
        let spooled = builder.finish().unwrap().summary().unwrap();
        assert_eq!(inline, spooled);
        assert_eq!(inline.created_at, Some(10));
        assert_eq!(inline.updated_at, Some(20));
        assert_eq!(inline.created_message_id.as_deref(), Some("a"));
        assert_eq!(inline.updated_message_id.as_deref(), Some("late"));
        assert_eq!(inline.searchable, 0);
        assert!(inline.title.is_none());
    }
}
