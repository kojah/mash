use super::*;
use crate::views::config::Build;
use crate::{provider::Provider, role::Role, source::Source};
use std::collections::BTreeSet;

fn document(native: bool) -> Document {
    let anchor = if native {
        "message"
    } else {
        "connector#document#notes#note"
    };
    Document {
        node_id: "node".into(),
        source: if native {
            Source::Provider(Provider::Codex)
        } else {
            Source::Connector("notes".into())
        },
        provider: native.then_some(Provider::Codex),
        source_id: (!native).then(|| "notes".into()),
        role: native.then_some(Role::User),
        object: if native {
            crate::source::ObjectType::native_message(Provider::Codex)
        } else {
            crate::source::ObjectType {
                role: crate::connectors::Role::Document,
                kind: "notes:note".into(),
            }
        },
        conversation_id: if native { "chat".into() } else { anchor.into() },
        project: None,
        repository: None,
        created_at: Some(100),
        tokens: vec!["needle".into()],
        unit: SearchUnit {
            anchor: anchor.into(),
            text: "needle".into(),
            content: "needle".into(),
            title: Some("Title".into()),
            enrichment: None,
            author_kind: None,
            sequence: 3,
            chat_created_at: Some(90),
            chat_updated_at: Some(110),
            ancestors: if native {
                crate::source::native_ancestors(Provider::Codex, "chat", Some("Title"))
            } else {
                vec![]
            },
            generation: Some("generation".into()),
        },
        vector: Some(vec![1.0, 0.0]),
    }
}

fn id(native: bool) -> String {
    if native {
        document_id("node", "message")
    } else {
        connector_id("node", "connector#document#notes#note")
    }
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
    for native in [true, false] {
        let literal = document_hit(&id(native), document(native), &active, None).unwrap();
        let ranked = document_hit(
            &id(native),
            document(native),
            &active,
            Some((&scope(), &Build::default(), false)),
        )
        .unwrap();
        assert_eq!(literal.id, ranked.id);
        assert_eq!(literal.source, ranked.source);
        assert_eq!(literal.ancestors, ranked.ancestors);
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
    for native in [true, false] {
        assert!(document_hit("wrong-id", document(native), &active, None).is_err());
        assert!(document_hit(&id(native), document(native), &BTreeSet::new(), None).is_err());
        for field in ["node", "source", "group", "tokens", "empty", "object"] {
            let mut d = document(native);
            match field {
                "node" => d.node_id = "other".into(),
                "source" => d.source = Source::Connector("other".into()),
                "group" => d.conversation_id = "other".into(),
                "tokens" => d.tokens.clear(),
                "empty" => d.unit.text.clear(),
                "object" => d.object.role = crate::connectors::Role::Container,
                _ => unreachable!(),
            }
            assert!(
                document_hit(&id(native), d, &active, None).is_err(),
                "{native}: {field}"
            );
        }
    }
}

#[test]
fn common_document_requires_current_dense_evidence_only_for_ranked_reads() {
    let active = BTreeSet::from(["node".into()]);
    for native in [true, false] {
        for field in ["generation", "missing", "dimension", "nan"] {
            let mut d = document(native);
            match field {
                "generation" => d.unit.generation = Some("old".into()),
                "missing" => d.vector = None,
                "dimension" => d.vector = Some(vec![1.0]),
                "nan" => d.vector = Some(vec![f64::NAN, 0.0]),
                _ => unreachable!(),
            }
            assert!(
                document_hit(
                    &id(native),
                    d,
                    &active,
                    Some((&scope(), &Build::default(), true))
                )
                .is_err(),
                "{native}: {field}"
            );
        }
        let mut d = document(native);
        d.vector = None;
        d.unit.generation = None;
        assert!(document_hit(&id(native), d, &active, None).is_ok());
    }
}

#[test]
fn common_document_keeps_source_metadata_and_anchor_namespaces_consistent() {
    let active = BTreeSet::from(["node".into()]);
    for field in [
        "provider",
        "source_id",
        "role",
        "author",
        "kind",
        "content",
        "group",
    ] {
        let mut d = document(true);
        match field {
            "provider" => d.provider = None,
            "source_id" => d.source_id = Some("notes".into()),
            "role" => d.role = None,
            "author" => d.unit.author_kind = Some(crate::connectors::AuthorKind::Human),
            "kind" => d.object.kind = "notes:note".into(),
            "content" => d.unit.content = "different".into(),
            "group" => {
                d.conversation_id.clear();
                d.unit.ancestors.clear();
            }
            _ => unreachable!(),
        }
        assert!(
            document_hit(&id(true), d, &active, None).is_err(),
            "{field}"
        );
    }
    for field in [
        "provider",
        "source_id",
        "role",
        "anchor",
        "anchor_source",
        "anchor_role",
        "anchor_id",
    ] {
        let mut d = document(false);
        match field {
            "provider" => d.provider = Some(Provider::Codex),
            "source_id" => d.source_id = None,
            "role" => d.role = Some(Role::User),
            "anchor" => d.unit.anchor = "unqualified".into(),
            "anchor_source" => d.unit.anchor = "connector#document#other#note".into(),
            "anchor_role" => d.unit.anchor = "connector#entry#notes#note".into(),
            "anchor_id" => d.unit.anchor = "connector#document#notes#".into(),
            _ => unreachable!(),
        }
        d.conversation_id.clone_from(&d.unit.anchor);
        let point_id = connector_id("node", &d.unit.anchor);
        assert!(
            document_hit(&point_id, d, &active, None).is_err(),
            "{field}"
        );
    }
}

#[test]
fn common_document_excluded_dense_units_are_lexical_only() {
    let active = BTreeSet::from(["node".into()]);
    for native in [true, false] {
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
            let mut d = document(native);
            if !vector {
                d.vector = None;
            }
            if !generation {
                d.unit.generation = None;
            }
            assert_eq!(
                document_hit(&id(native), d, &active, Some((&scope(), &build, lexical))).is_ok(),
                accepted
            );
        }
    }
    let mut d = document(true);
    d.role = Some(Role::Assistant);
    d.vector = None;
    d.unit.generation = None;
    let build = Build {
        dense_content: crate::views::config::DenseContent::UserMessages,
        ..Build::default()
    };
    assert!(document_hit(&id(true), d, &active, Some((&scope(), &build, true))).is_ok());
}
