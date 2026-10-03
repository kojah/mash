use super::NormalizedMessage;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Summary {
    pub(crate) created_at: Option<i64>,
    pub(crate) updated_at: Option<i64>,
    pub(crate) title: Option<String>,
    pub(crate) searchable: u64,
}

impl Summary {
    pub(super) fn observe(&mut self, message: &NormalizedMessage) {
        if let Some(timestamp) = message.created_at {
            self.created_at = Some(
                self.created_at
                    .map_or(timestamp, |previous| previous.min(timestamp)),
            );
            self.updated_at = Some(
                self.updated_at
                    .map_or(timestamp, |previous| previous.max(timestamp)),
            );
        }
        let projection = message
            .search_projection
            .as_deref()
            .unwrap_or(&message.content);
        self.searchable += u64::from(!projection.is_empty());
        if self.title.is_none() && message.role == crate::role::Role::User {
            self.title = crate::search_text::title(projection);
        }
    }
}
