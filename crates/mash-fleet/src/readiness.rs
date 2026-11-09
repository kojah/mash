//! Read-only release checks over the installed MCP's public contract.
use rmcp::model::CallToolResult;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Stamp {
    pub contract_fingerprint: String,
    #[serde(skip)]
    pub build_revision: String,
}

pub async fn expected(repository: &std::path::Path) -> Result<Stamp, String> {
    let bytes = std::fs::read(repository.join("plugins/mash/skills/mash-history/contract.json"))
        .map_err(|_| "cannot read generated history contract stamp")?;
    let mut stamp: Stamp =
        serde_json::from_slice(&bytes).map_err(|_| "invalid history contract stamp")?;
    let revision = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repository)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| "release identity lookup timed out")?
    .map_err(|_| "cannot read release identity")?;
    if !revision.status.success() {
        return Err("deployment repository has no verified Git revision".into());
    }
    String::from_utf8(revision.stdout)
        .map_err(|_| "invalid Git revision")?
        .trim()
        .clone_into(&mut stamp.build_revision);
    Ok(stamp)
}

#[derive(Deserialize)]
struct Runtime {
    build_revision: String,
    contract_fingerprint: String,
    contract_matches: Option<bool>,
}

#[derive(Deserialize)]
struct Help {
    runtime: Runtime,
}

pub fn help(result: &CallToolResult, expected: &Stamp) -> Result<(), String> {
    let help: Help = decode(result)?;
    if help.runtime.contract_matches != Some(true)
        || help.runtime.contract_fingerprint != expected.contract_fingerprint
        || help.runtime.build_revision != expected.build_revision
    {
        return Err(
            "installed MCP build or contract differs; install the matching release and reconnect"
                .into(),
        );
    }
    Ok(())
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Models {
    Available,
    NotRequired,
    Unavailable,
}

#[derive(Deserialize)]
pub struct Status {
    pub ready: bool,
    pub query_models: Models,
    shared_error: Option<String>,
    configuration: Configuration,
    shared_index: Option<Freshness>,
    shared: Option<Publication>,
}

#[derive(Deserialize)]
struct Configuration {
    shared: Option<Node>,
}
#[derive(Deserialize)]
struct Node {
    node_id: String,
}
#[derive(Deserialize)]
struct Freshness {
    nodes: Vec<Member>,
}
#[derive(Deserialize)]
struct Member {
    node_id: String,
    lexical_current: bool,
    views_current: bool,
}

#[derive(Deserialize)]
struct Publication {
    state: PublicationState,
}
#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum PublicationState {
    Pending,
    Published,
}

pub fn status(result: &CallToolResult, shared: bool) -> Result<Status, String> {
    let status: Status = decode(result)?;
    if !status.ready || status.query_models == Models::Unavailable {
        return Err("installed node is not query-ready; inspect status and refresh or install models explicitly".into());
    }
    if shared {
        let node = status
            .configuration
            .shared
            .as_ref()
            .ok_or("shared deployment check needs a configured publishing node")?;
        let member = status.shared_index.as_ref().and_then(|index| {
            index
                .nodes
                .iter()
                .find(|member| member.node_id == node.node_id)
        });
        if status.shared_error.is_some()
            || !status
                .shared
                .as_ref()
                .is_some_and(|publication| publication.state == PublicationState::Published)
            || !member.is_some_and(|member| {
                member.lexical_current
                    && (status.query_models != Models::Available || member.views_current)
            })
        {
            return Err("this node's shared publication is missing or stale; refresh explicitly before declaring it ready".into());
        }
    }
    Ok(status)
}

#[derive(Deserialize)]
struct SearchResult {
    shared_error: Option<String>,
    shared: bool,
}

pub fn search(result: &CallToolResult, shared: bool) -> Result<(), String> {
    let result: SearchResult = decode(result)?;
    if result.shared_error.is_some() || (shared && !result.shared) {
        return Err("read-only smoke search could not verify the shared index".into());
    }
    Ok(())
}

fn decode<T: serde::de::DeserializeOwned>(result: &CallToolResult) -> Result<T, String> {
    if result.is_error == Some(true) {
        return Err("Mash returned a tool error; inspect results".into());
    }
    serde_json::from_value(
        result
            .structured_content
            .clone()
            .ok_or("Mash check lacks structured content")?,
    )
    .map_err(|_| "Mash check returned an incompatible contract".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> serde_json::Value {
        json!({"ready":true,"query_models":"available","configuration":{"shared":{"node_id":"one"}},"shared":{"state":"published"},"shared_index":{"nodes":[{"node_id":"one","lexical_current":true,"views_current":true}]},"runtime":{"build_revision":"revision","contract_fingerprint":"current","contract_matches":true}})
    }

    #[test]
    fn deployment_rejects_stale_contracts_and_nonready_successful_status() {
        let fixture = fixture();
        let expected = Stamp {
            contract_fingerprint: "current".into(),
            build_revision: "revision".into(),
        };
        assert!(help(&CallToolResult::structured(fixture.clone()), &expected).is_ok());
        assert!(
            help(
                &CallToolResult::structured(fixture.clone()),
                &Stamp {
                    contract_fingerprint: "old".into(),
                    build_revision: "revision".into()
                }
            )
            .is_err()
        );
        assert!(
            help(
                &CallToolResult::structured(fixture.clone()),
                &Stamp {
                    contract_fingerprint: "current".into(),
                    build_revision: "old".into()
                }
            )
            .is_err()
        );
        assert!(status(&CallToolResult::structured(fixture.clone()), true).is_ok());
        for (field, value) in [
            ("ready", json!(false)),
            ("query_models", json!("unavailable")),
            ("shared_error", json!("offline")),
        ] {
            let mut changed = fixture.clone();
            changed[field] = value;
            assert!(status(&CallToolResult::structured(changed), true).is_err());
        }
        let mut changed = fixture.clone();
        changed["shared_index"]["nodes"][0]["views_current"] = json!(false);
        assert!(status(&CallToolResult::structured(changed), true).is_err());
        let mut changed = fixture.clone();
        changed["runtime"]["build_revision"] = json!("old");
        assert!(help(&CallToolResult::structured(changed), &expected).is_err());
        assert!(
            search(
                &CallToolResult::structured(json!({"shared":false,"shared_error":"offline"})),
                true
            )
            .is_err()
        );
    }
}
