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
    fn chat_evidence_roundtrips_and_invalidates_changed_history(id in ".{0,100}", count in 0u64..100) {
        let mut projections = Projections::default();
        let native = id.clone();
        projections.insert(native.clone(), evidence("native", count, true));
        let encoded = serde_json::to_string(&projections).unwrap();
        let decoded: Projections = serde_json::from_str(&encoded).unwrap();
        prop_assert_eq!(&decoded, &projections);
        prop_assert_eq!(decoded.embeddings(), count);
        prop_assert!(decoded.complete());
        let current = BTreeMap::from([(native.clone(), "changed".into())]);
        projections.retain_matching(&current);
        prop_assert!(projections.get(&native).is_none());
        prop_assert_eq!(projections.embeddings(), 0);
        prop_assert!(projections.complete());
    }
}

#[test]
fn chat_merge_preserves_preferred_current_evidence() {
    let scope = "chat".to_owned();
    let current = BTreeMap::from([("chat".into(), "current".into())]);
    let mut preferred = Projections::default();
    preferred.insert(scope.clone(), evidence("current", 3, true));
    let mut alternate = Projections::default();
    alternate.insert(scope.clone(), evidence("current", 0, false));
    preferred.merge_matching(&alternate, &current);
    assert_eq!(preferred.get(&scope), Some(&evidence("current", 3, true)));
    preferred.insert(scope.clone(), evidence("stale", 3, true));
    preferred.merge_matching(&alternate, &current);
    assert_eq!(preferred.get(&scope), Some(&evidence("current", 0, false)));
    assert!(!preferred.complete());
    alternate.insert("deleted".into(), evidence("old", 20, true));
    alternate.insert("changed".into(), evidence("old", 20, true));
    preferred.merge_matching(&alternate, &current);
    assert!(preferred.get("deleted").is_none());
    assert!(preferred.get("changed").is_none());
    preferred.retain_matching(&BTreeMap::new());
    assert!(preferred.get(&scope).is_none());
}
