use super::*;
use crate::views::config::Build;
use crate::{provider::Provider, role::Role};
use std::collections::BTreeSet;

fn document() -> Document {
    Document {
        node_id: "node".into(),
        provider: Provider::Codex,
        role: Role::User,
        conversation_id: "chat".into(),
        project: None,
        repository: None,
        created_at: Some(100),
        tokens: vec!["needle".into()],
        unit: SearchUnit {
            anchor: "message".into(),
            text: "needle".into(),
            content: "needle".into(),
            title: Some("Title".into()),
            enrichment: None,
            sequence: 3,
            chat_created_at: Some(90),
            chat_updated_at: Some(110),
            generation: Some("generation".into()),
        },
        vector: Some(vec![1.0, 0.0]),
        embedding_fingerprint: None,
    }
}

fn id() -> String {
    document_id("node", "message")
}

fn scope() -> Scope {
    Scope {
        group_id: "group".into(),
        node_id: "node".into(),
        generation: "generation".into(),
        dimensions: 2,
    }
}

#[test]
fn common_document_preserves_evidence_in_literal_and_ranked_search() {
    let active = BTreeSet::from(["node".into()]);
    {
        let literal = document_hit(&id(), document(), &active, None).unwrap();
        let ranked = document_hit(
            &id(),
            document(),
            &active,
            Some((&scope(), &Build::default(), false)),
        )
        .unwrap();
        assert_eq!(literal.id, ranked.id);
        assert_eq!(literal.provider, ranked.provider);
        assert_eq!(literal.content, "needle");
        assert_eq!(ranked.content, "needle");
        assert_eq!(literal.sequence, Some(3));
        assert_eq!(ranked.sequence, Some(3));
        assert_eq!(literal.metadata.title.as_deref(), Some("Title"));
        assert_eq!(ranked.metadata.updated_at, Some(110));
    }
}

#[test]
fn common_document_rejects_mismatched_identity_and_projection_evidence() {
    let active = BTreeSet::from(["node".into()]);
    {
        assert!(document_hit("wrong-id", document(), &active, None).is_err());
        assert!(document_hit(&id(), document(), &BTreeSet::new(), None).is_err());
        for field in ["node", "anchor", "group", "tokens", "empty"] {
            let mut d = document();
            match field {
                "node" => d.node_id = "other".into(),
                "anchor" => d.unit.anchor.clear(),
                "group" => d.conversation_id.clear(),
                "tokens" => d.tokens.clear(),
                "empty" => d.unit.text.clear(),
                _ => unreachable!(),
            }
            assert!(document_hit(&id(), d, &active, None).is_err(), "{field}");
        }
    }
}

#[test]
fn common_document_requires_current_dense_evidence_only_for_ranked_reads() {
    let active = BTreeSet::from(["node".into()]);
    {
        for field in ["generation", "missing", "dimension", "nan"] {
            let mut d = document();
            match field {
                "generation" => d.unit.generation = Some("old".into()),
                "missing" => d.vector = None,
                "dimension" => d.vector = Some(vec![1.0]),
                "nan" => d.vector = Some(vec![f64::NAN, 0.0]),
                _ => unreachable!(),
            }
            assert!(
                document_hit(&id(), d, &active, Some((&scope(), &Build::default(), true))).is_err(),
                "{field}"
            );
        }
        let mut d = document();
        d.vector = None;
        d.unit.generation = None;
        assert!(document_hit(&id(), d, &active, None).is_ok());
    }
}

#[test]
fn common_document_requires_message_content_and_chat_identity() {
    let active = BTreeSet::from(["node".into()]);
    for field in ["content", "group"] {
        let mut d = document();
        match field {
            "content" => d.unit.content = "different".into(),
            "group" => {
                d.conversation_id.clear();
            }
            _ => unreachable!(),
        }
        assert!(document_hit(&id(), d, &active, None).is_err(), "{field}");
    }
}

#[test]
fn common_document_excluded_dense_units_are_lexical_only() {
    let active = BTreeSet::from(["node".into()]);
    {
        let build = Build {
            representations: crate::views::config::Representations::LexicalOnly,
            ..Build::default()
        };
        for (vector, generation, lexical, accepted) in [
            (false, false, true, true),
            (true, false, true, false),
            (false, true, true, false),
            (false, false, false, false),
        ] {
            let mut d = document();
            if !vector {
                d.vector = None;
            }
            if !generation {
                d.unit.generation = None;
            }
            assert_eq!(
                document_hit(&id(), d, &active, Some((&scope(), &build, lexical))).is_ok(),
                accepted
            );
        }
    }
    let mut d = document();
    d.role = Role::Assistant;
    d.vector = None;
    d.unit.generation = None;
    let build = Build {
        dense_content: crate::views::config::DenseContent::UserMessages,
        ..Build::default()
    };
    assert!(document_hit(&id(), d, &active, Some((&scope(), &build, true))).is_ok());
}
