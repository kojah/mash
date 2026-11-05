use super::*;
use proptest::prelude::*;

fn evidence(fingerprint: &str, embeddings: u64, complete: bool) -> ProjectionEvidence {
    ProjectionEvidence {
        fingerprint: fingerprint.into(),
        embeddings,
        complete,
    }
}

proptest! {
    #[test]
    fn projection_scopes_roundtrip_without_aliasing(id in ".{0,100}", count in 0u64..100) {
        let mut projections = Projections::default();
        let native = ProjectionScope::NativeChat(id.clone());
        let connector = ProjectionScope::ConnectorUnit(id.clone());
        projections.insert(native.clone(), evidence("native", count, true));
        projections.insert(connector.clone(), evidence("connector", 1, false));
        let encoded = serde_json::to_string(&projections).unwrap();
        let decoded: Projections = serde_json::from_str(&encoded).unwrap();
        prop_assert_eq!(&decoded, &projections);
        prop_assert_eq!(decoded.embeddings(), count + 1);
        prop_assert!(!decoded.complete());
        let current = BTreeMap::from([(native.clone(), "changed".into())]);
        projections.retain_matching(&current, ProjectionFamily::Native);
        prop_assert!(projections.get(&native).is_none());
        prop_assert_eq!(projections.get(&connector), Some(&evidence("connector", 1, false)));
        projections.retain_matching(&BTreeMap::new(), ProjectionFamily::Connector);
        prop_assert_eq!(projections.embeddings(), 0);
        prop_assert!(projections.complete());
    }
}

#[test]
fn projection_merge_preserves_preferred_current_evidence_and_destination_scope() {
    let scope = ProjectionScope::NativeChat("chat".into());
    let connector = ProjectionScope::ConnectorUnit("chat".into());
    let current = BTreeMap::from([("chat".into(), "current".into())]);
    let mut preferred = Projections::default();
    preferred.insert(scope.clone(), evidence("current", 3, true));
    preferred.insert(connector.clone(), evidence("local-connector", 1, true));
    let mut alternate = Projections::default();
    alternate.insert(scope.clone(), evidence("current", 0, false));
    alternate.insert(connector.clone(), evidence("other-connector", 1, true));
    preferred.merge_native(&alternate, &current);
    assert_eq!(preferred.get(&scope), Some(&evidence("current", 3, true)));
    assert_eq!(
        preferred.get(&connector),
        Some(&evidence("local-connector", 1, true))
    );
    preferred.insert(scope.clone(), evidence("stale", 3, true));
    preferred.merge_native(&alternate, &current);
    assert_eq!(preferred.get(&scope), Some(&evidence("current", 0, false)));
    preferred.retain_matching(&BTreeMap::new(), ProjectionFamily::Native);
    assert!(preferred.get(&scope).is_none());
    assert!(preferred.get(&connector).is_some());
    assert!(
        serde_json::from_str::<Projections>(
            r#"{"unknown:chat":{"fingerprint":"x","embeddings":0,"complete":true}}"#
        )
        .is_err()
    );
}
