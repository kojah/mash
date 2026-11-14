//! Conservative local language validation before inference or network access.
use crate::Error;

pub(super) fn validate(query: &str) -> Result<(), Error> {
    if query.chars().any(|character| {
        character.is_alphabetic()
            && !matches!(u32::from(character), 0..=0x024f | 0x1e00..=0x1eff | 0x2c60..=0x2c7f | 0xa720..=0xa7ff | 0xab30..=0xab6f)
    }) {
        return Err(Error::UnsupportedScript);
    }
    // Names, URLs and short technical phrases do not provide enough linguistic
    // evidence. Never treat an uncertain prediction as an unsupported language.
    let words = query
        .split_whitespace()
        .filter(|word| word.chars().all(char::is_alphabetic))
        .take(3)
        .count();
    if words >= 3
        && let Some(info) = whatlang::detect(query)
        && info.lang() != whatlang::Lang::Eng
        && info.is_reliable()
        && info.confidence() >= 0.9
    {
        return Err(Error::UnsupportedLanguage(info.lang().eng_name()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confident_non_english_queries_are_rejected() {
        for query in [
            "wie konfiguriere ich den gemeinsamen Suchindex",
            "come configurare l'indice condiviso di ricerca",
            "jak skonfigurować wspólny indeks wyszukiwania",
            "quiero encontrar las conversaciones recientes sobre el índice de búsqueda compartido",
            "je voudrais retrouver les conversations récentes sur notre moteur de recherche partagé",
        ] {
            let error = validate(query).expect_err(query);
            assert!(error.to_string().contains("Rephrase"), "{error:?}");
        }
    }

    #[test]
    fn english_and_ambiguous_technical_queries_remain_usable() {
        for query in [
            "cBioPortal",
            "café caching",
            "résumé",
            "PR 180",
            "123",
            "voyage-code-4",
            "naïve vector search",
            "https://github.com/mashium/mash/pull/180",
            "rust tokio-postgres redis-rs axum",
            "authentication authorization configuration",
            "configuration documentation migration",
            "postgres connection pooling timeout retry",
            "semantic search reranking embeddings",
            "initial indexing progress remaining eta",
            "sqlite vector search performance",
            "model inference synchronous asynchronous",
            "what did we discuss about the personal macbook low power mode",
            "how did we configure Redis for the shared search index",
            "why is local search slower than the shared index",
            "give me examples of recent cbioportal conversations",
            "Find conversations where I discussed changing the shared index backend",
        ] {
            assert!(validate(query).is_ok(), "{query}");
        }
    }
}
