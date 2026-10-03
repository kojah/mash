use super::*;
use crate::provider::Provider;
use crate::shared::protocol::{NodeState, ScanReport};
use crate::silver::certificate::INTERPRETATION_VERSION;

#[tokio::test]
#[ignore = "requires disposable Qdrant"]
async fn qdrant_silver_scan_reports() {
    contract(Service::qdrant(384).await).await;
}

async fn state(service: &Service) -> NodeState {
    serde_json::from_value(service.get("state", FIRST).await).unwrap()
}

fn delta(revision: i64, base: i64) -> Vec<Value> {
    vec![
        json!({"type":"start","instance":"snapshot-instance","revision":revision,
        "base_revision":base,"snapshot":false,"forgets":[]}),
        json!({"type":"reconcile","provider":"codex","cutoff":123}),
        json!({"type":"scan","provider":"codex","cutoff":123,
            "interpretation_version":INTERPRETATION_VERSION,"authoritative":true}),
        json!({"type":"scan","provider":"claude-code","cutoff":null,
            "interpretation_version":INTERPRETATION_VERSION,"authoritative":false}),
        json!({"type":"finish"}),
    ]
}

async fn contract(service: Service) {
    let mut initial = snapshot(1);
    for record in initial[2]["records"].as_array_mut().unwrap() {
        record["vector"] = Value::Null;
        record["generation"] = Value::Null;
    }
    service
        .upload(FIRST, &initial)
        .await
        .error_for_status()
        .unwrap();
    assert_eq!(state(&service).await.silver_scans, Vec::<ScanReport>::new());
    let events = delta(2, 1);
    assert_eq!(
        service.upload(FIRST, &events[..4]).await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(state(&service).await.silver_scans, Vec::<ScanReport>::new());
    service
        .upload(FIRST, &events)
        .await
        .error_for_status()
        .unwrap();
    let accepted = state(&service).await;
    assert_eq!(accepted.revision, 2);
    assert_eq!(accepted.silver_scans.len(), 2);
    assert!(accepted.silver_scans.contains(&ScanReport {
        provider: Provider::Codex,
        interpretation_version: INTERPRETATION_VERSION,
        authoritative: true,
        cutoff: Some(123)
    }));
    assert!(accepted.silver_scans.contains(&ScanReport {
        provider: Provider::ClaudeCode,
        interpretation_version: INTERPRETATION_VERSION,
        authoritative: false,
        cutoff: None
    }));
    for defect in 0..6 {
        let mut invalid = delta(3, 2);
        match defect {
            0 => invalid[2]["interpretation_version"] = json!(INTERPRETATION_VERSION + 1),
            1 => invalid[2]["cutoff"] = json!(124),
            2 => invalid[2]["authoritative"] = json!(false),
            3 => {
                invalid.remove(1);
            }
            4 => {
                invalid.insert(4, invalid[2].clone());
            }
            _ => {
                invalid.insert(
                    4,
                    json!({"type":"seen","provider":"claude-code","hashes":[]}),
                );
            }
        }
        assert_eq!(
            service.upload(FIRST, &invalid).await.status(),
            StatusCode::BAD_REQUEST,
            "defect {defect}"
        );
        assert_eq!(state(&service).await, accepted);
    }
    // A later legacy delta has no scan evidence; never retain a misleading report.
    service
        .upload(FIRST, &[delta(3, 2)[0].clone(), json!({"type":"finish"})])
        .await
        .error_for_status()
        .unwrap();
    assert_eq!(state(&service).await.silver_scans, Vec::<ScanReport>::new());
    service
        .upload(FIRST, &delta(4, 3))
        .await
        .error_for_status()
        .unwrap();
    service
        .request("forget", FIRST)
        .json(&json!({"instance":"snapshot-instance","revision":5,"ids":["conversation"]}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    assert_eq!(state(&service).await.silver_scans, Vec::<ScanReport>::new());
    service.cleanup().await;
}
