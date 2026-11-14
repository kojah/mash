//! The model behind the dashboard's assistant. There is none unless the
//! configuration names one: Mash itself never talks to a language model.
use crate::AppError;
use serde::{Deserialize, Serialize};

/// Any service that speaks the `OpenAI` chat-completions format: a provider's
/// own API, a gateway such as `OpenRouter`, or a model server on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Assistant {
    /// The API's base, such as `https://openrouter.ai/api/v1`; requests go to
    /// `chat/completions` beneath it.
    pub(crate) base_url: String,
    /// The environment variable that holds the API key.
    pub(crate) token_env: String,
    /// The model's name as that service knows it.
    pub model: String,
}

impl Assistant {
    pub(crate) fn validate(&self) -> Result<(), AppError> {
        super::embeddings::validate_service(&self.base_url, &self.token_env, "assistant")?;
        if self.model.trim().is_empty() || self.model.len() > 200 {
            return Err(AppError::configuration(
                "assistant model must be a name of at most 200 characters",
            ));
        }
        Ok(())
    }

    /// The API key: the variable `token_env` names, or the copy saved for it
    /// under Mash's own directory, which a supervised dashboard can read
    /// without a change to its service.
    pub fn token(&self) -> Option<String> {
        super::secret(&self.token_env)
    }

    /// Where chat requests are sent.
    pub fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }
}

#[cfg(test)]
mod tests {
    use super::Assistant;

    fn assistant(base_url: &str, token_env: &str, model: &str) -> Assistant {
        Assistant {
            base_url: base_url.into(),
            token_env: token_env.into(),
            model: model.into(),
        }
    }

    #[test]
    fn a_gateway_or_a_local_server_is_accepted_and_plain_http_elsewhere_is_not() {
        let gateway = assistant("https://openrouter.ai/api/v1", "MASH_ASSISTANT_KEY", "a/b");
        gateway.validate().unwrap();
        assert_eq!(
            gateway.endpoint(),
            "https://openrouter.ai/api/v1/chat/completions"
        );
        let local = assistant("http://127.0.0.1:11434/v1/", "OLLAMA_KEY", "llama");
        local.validate().unwrap();
        assert_eq!(
            local.endpoint(),
            "http://127.0.0.1:11434/v1/chat/completions"
        );
        for refused in [
            assistant("http://example.com/v1", "KEY", "model"),
            assistant("https://user:secret@example.com/v1", "KEY", "model"),
            assistant("https://example.com/v1", "not a name", "model"),
            assistant("https://example.com/v1", "KEY", " "),
        ] {
            assert!(refused.validate().is_err(), "{refused:?}");
        }
    }

    #[test]
    fn the_table_is_strict_and_optional() {
        let with = "version = 1\nproviders = {}\n[assistant]\nbase_url = 'https://openrouter.ai/api/v1'\ntoken_env = 'KEY'\nmodel = 'a/b'\n";
        let parsed = super::super::document::parse_document(with.as_bytes()).unwrap();
        assert_eq!(parsed.assistant.unwrap().model, "a/b");
        let without =
            super::super::document::parse_document(b"version = 1\nproviders = {}\n").unwrap();
        assert!(without.assistant.is_none());
        let unknown = format!("{with}api_key = 'sk-secret'\n");
        assert!(super::super::document::parse_document(unknown.as_bytes()).is_err());
    }
}
