//! Query policy failures independent of CLI/MCP/HTTP error envelopes.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    InvalidQuery,
    CommonWords,
    UnsupportedScript,
    UnsupportedLanguage(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidQuery => formatter.write_str(
                "search query must contain letters or numbers and be at most 16384 bytes",
            ),
            Self::CommonWords => formatter.write_str(
                "enter a more specific search query; common words alone do not identify a topic",
            ),
            Self::UnsupportedScript => formatter.write_str(
                "the current search models support English queries; queries in other scripts are unsupported",
            ),
            Self::UnsupportedLanguage(language) => write!(formatter,
                "the current search models support English queries; detected {language}. Rephrase the query in English"),
        }
    }
}

impl std::error::Error for Error {}
