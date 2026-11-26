//! Public process graph derived from the immutable definition used by a Heart instance.
use super::{CircleProcess, CircleProcessStep, step_name};
use reqwest::{Client, Url};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    time::Duration,
};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize)]
pub struct DefinitionStep {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub next: Vec<String>,
}

// Definitions can contain Lua programs, queue names and private metadata.
// Project only structural identifiers, node kinds and their target identifiers.
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 120
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
}

fn graph(nodes: &Value) -> Option<Vec<DefinitionStep>> {
    let nodes = nodes.as_object()?;
    if nodes.is_empty() || nodes.len() > 256 {
        return None;
    }
    let mut steps = BTreeMap::new();
    let mut queue = VecDeque::new();
    for (id, node) in nodes {
        if !identifier(id) {
            return None;
        }
        let kind = node["type"].as_str()?;
        let mut next = Vec::new();
        match kind {
            "start" | "task" | "receive" | "user-task" | "external-task" | "join" => {
                next.push(node["next"].as_str()?)
            }
            "fork" => {
                for target in node["branches"].as_object()?.values() {
                    next.push(target.as_str()?);
                }
            }
            "gate" => {
                for condition in node["conditions"].as_array()? {
                    next.push(condition["next"].as_str()?);
                }
                if let Some(target) = node["default"].as_str() {
                    next.push(target);
                }
            }
            "end" => {}
            _ => return None,
        }
        if next.iter().any(|target| !nodes.contains_key(*target)) {
            return None;
        }
        if kind == "start" {
            queue.push_back(id.clone());
        }
        steps.insert(
            id.clone(),
            DefinitionStep {
                id: id.clone(),
                name: step_name(id),
                kind: kind.into(),
                next: next.into_iter().map(str::to_owned).collect(),
            },
        );
    }
    let mut ordered = Vec::new();
    while let Some(id) = queue.pop_front() {
        if let Some(step) = steps.remove(&id) {
            queue.extend(step.next.iter().cloned());
            ordered.push(step);
        }
    }
    ordered.extend(steps.into_values());
    Some(ordered)
}

async fn fetch(client: &Client, base: &str, segments: &[&str]) -> Option<Value> {
    let mut url = Url::parse(base).ok()?;
    url.path_segments_mut()
        .ok()?
        .pop_if_empty()
        .extend(segments);
    let mut response = client
        .get(url)
        .timeout(Duration::from_secs(3))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).ok()
}

fn uuid(value: &Value) -> Option<Uuid> {
    value.as_str()?.parse().ok()
}

fn transition_history(
    events: &[Value],
    instance: Uuid,
    node_ids: &BTreeSet<&str>,
) -> Option<Vec<CircleProcessStep>> {
    events
        .iter()
        .rev()
        .take(super::STEP_LIMIT)
        .rev()
        .map(|event| {
            let from = event["from_node"].as_str()?;
            if uuid(&event["instance_id"])? != instance || !node_ids.contains(from) {
                return None;
            }
            // Timestamps are public. Parse before exposing a string received from Heart.
            let occurred_at = event["transitioned_at"]
                .as_str()
                .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                .map(|time| time.to_rfc3339());
            Some(CircleProcessStep {
                task: None,
                node_id: Some(from.into()),
                name: step_name(from),
                status: "completed".into(),
                occurred_at,
            })
        })
        .collect()
}

impl CircleProcess {
    pub(crate) async fn load_definition(&mut self, client: &Client, base: &str) {
        // Use only the configured address. A pilot pinned to another deployment
        // must never resolve by name against the current deployment.
        if self.kind == "pilot"
            && self.engine_url.as_deref().is_some_and(|url| {
                !url.is_empty() && url.trim_end_matches('/') != base.trim_end_matches('/')
            })
        {
            return;
        }
        let _ = self.resolve_definition(client, base).await;
    }

    async fn resolve_definition(&mut self, client: &Client, base: &str) -> Option<()> {
        let runtime = match self.runtime.as_str() {
            "v1" => "v1",
            "v2" => "v2",
            _ => return None,
        };
        let instance_id = self.instance_id.as_deref()?;
        let instance_uuid: Uuid = instance_id.parse().ok()?;
        let instance = fetch(client, base, &["api", runtime, "instances", instance_id]).await?;
        if uuid(&instance["id"])? != instance_uuid
            || instance["namespace"].as_str()? != self.definition_namespace
            || (runtime == "v2" && instance["runtime"] != "v2")
        {
            return None;
        }
        let definition_id = uuid(&instance["definition_id"])?;
        let namespace = self.definition_namespace.as_str();
        let name = self.definition_name.as_str();
        if !identifier(namespace) || !identifier(name) {
            return None;
        }
        let version = match self
            .definition_version
            .as_deref()
            .filter(|version| !version.is_empty())
        {
            Some(version) => version.to_owned(),
            // Old v1 starts could omit the version. Match the instance's UUID,
            // never a latest/default version from the deployment or binding.
            None if runtime == "v1" => {
                let versions =
                    fetch(client, base, &["api", "v1", "definitions", namespace, name]).await?;
                versions
                    .as_array()?
                    .iter()
                    .find(|entry| uuid(&entry["id"]) == Some(definition_id))?["version"]
                    .as_str()?
                    .to_owned()
            }
            None => return None,
        };
        let response = fetch(
            client,
            base,
            &["api", runtime, "definitions", namespace, name, &version],
        )
        .await?;
        if uuid(&response["id"])? != definition_id {
            self.definition_version = None;
            return None;
        }
        let definition = if runtime == "v2" {
            &response["definition"]
        } else {
            &response
        };
        if definition["namespace"] != namespace
            || definition["name"] != name
            || definition["version"] != version
            || (runtime == "v2" && definition["runtime"] != "v2")
        {
            return None;
        }
        let steps = graph(&definition["nodes"])?;
        let node_ids: BTreeSet<_> = steps.iter().map(|node| node.id.as_str()).collect();
        let status = instance["status"].as_str()?;
        let active: BTreeSet<&str> = match (runtime, status) {
            ("v1", "running" | "waiting") => {
                [instance["current_node"].as_str()?].into_iter().collect()
            }
            ("v2", "running" | "waiting") => instance["active_activations"]
                .as_array()?
                .iter()
                .filter(|a| matches!(a["status"].as_str(), Some("waiting" | "ready" | "leased")))
                .map(|a| a["node_id"].as_str())
                .collect::<Option<_>>()?,
            (_, "completed" | "failed" | "cancelled") => BTreeSet::new(),
            _ => return None,
        };
        if !active.is_subset(&node_ids) {
            return None;
        }
        if runtime == "v1"
            && let Some(history) = fetch(
                client,
                base,
                &["api", runtime, "instances", instance_id, "transitions"],
            )
            .await
            && let Some(events) = history.as_array()
            && let Some(transitions) = transition_history(events, instance_uuid, &node_ids)
        {
            self.history = transitions;
            self.history_truncated = events.len() > super::STEP_LIMIT;
        }
        if self.kind != "feedback" {
            self.status = status.into();
        }
        self.active_node_ids = Some(active.into_iter().map(str::to_owned).collect());
        self.definition_version = Some(version);
        self.definition_steps = Some(steps);
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn graph_shows_all_alternatives_and_drops_executable_or_private_content() {
        let nodes = json!({
            "start":{"type":"start","next":"decision"},
            "decision":{"type":"gate","conditions":[{"when":"SECRET","next":"review"}],"default":"end"},
            "review":{"type":"user-task","description":"SECRET","assignee_metadata_key":"SECRET","next":"end"},
            "end":{"type":"end"}
        });
        let graph = graph(&nodes).unwrap();
        assert_eq!(graph[0].id, "start");
        assert_eq!(graph[1].next, ["review", "end"]);
        assert!(!serde_json::to_string(&graph).unwrap().contains("SECRET"));
    }
    fn item(runtime: &str, instance: Uuid) -> CircleProcess {
        CircleProcess {
            id: "process".into(),
            kind: "process".into(),
            title: "test".into(),
            channel_id: String::new(),
            channel_name: String::new(),
            status: "waiting".into(),
            definition_name: "review".into(),
            runtime: runtime.into(),
            definition_namespace: "sproyt".into(),
            definition_version: None,
            definition_steps: None,
            active_node_ids: None,
            instance_id: Some(instance.to_string()),
            engine_url: None,
            steps: Vec::new(),
            history: Vec::new(),
            steps_truncated: false,
            history_truncated: false,
            remaining_steps: None,
            projection: true,
        }
    }

    #[tokio::test]
    async fn legacy_unpinned_instance_uses_its_own_definition_id_and_safe_transitions() {
        use axum::{Json, Router, routing::get};
        let instance = Uuid::now_v7();
        let old = Uuid::now_v7();
        let later = Uuid::now_v7();
        let app=Router::new()
            .route(&format!("/api/v1/instances/{instance}"),get(move ||async move{
                Json(json!({"id":instance,"definition_id":old,"namespace":"sproyt","status":"waiting","current_node":"review","metadata":{"SECRET":"SECRET"}}))
            }))
            .route("/api/v1/definitions/sproyt/review",get(move ||async move{
                Json(json!([{"id":later,"version":"9.0.0"},{"id":old,"version":"1.0.0"}]))
            }))
            .route("/api/v1/definitions/sproyt/review/1.0.0",get(move ||async move{
                Json(json!({"id":old,"namespace":"sproyt","name":"review","version":"1.0.0","nodes":{
                    "start":{"type":"start","next":"review"},"review":{"type":"receive","correlation_value":"SECRET","next":"end"},"end":{"type":"end"}
                }}))
            }))
            .route(&format!("/api/v1/instances/{instance}/transitions"),get(move ||async move {
                Json(json!([{"instance_id":instance,"from_node":"start","to_node":"review","metadata_snapshot":{"SECRET":true},"transitioned_at":"2026-11-20T10:00:00Z"}]))
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mut row = item("v1", instance);
        row.load_definition(&Client::new(), &base).await;
        assert_eq!(row.definition_version.as_deref(), Some("1.0.0"));
        assert_eq!(row.active_node_ids.unwrap(), ["review"]);
        assert_eq!(row.history[0].node_id.as_deref(), Some("start"));
        assert!(
            !serde_json::to_string(&row.definition_steps.unwrap())
                .unwrap()
                .contains("SECRET")
        );
        server.abort();
    }

    #[tokio::test]
    async fn parallel_runtime_requires_the_same_immutable_definition_and_lists_both_activations() {
        use axum::{Json, Router, routing::get};
        let instance = Uuid::now_v7();
        let definition = Uuid::now_v7();
        let other = Uuid::now_v7();
        let make_definition = move |id: Uuid, version: &str| {
            json!({"id":id,"definition":{
                "namespace":"sproyt","name":"review","version":version,"runtime":"v2","nodes":{
                    "start":{"type":"start","next":"split"},
                    "split":{"type":"fork","branches":{"a":"first","b":"second"},"join":"join"},
                    "first":{"type":"user-task","next":"join"},"second":{"type":"user-task","next":"join"},
                    "join":{"type":"join","next":"end"},"end":{"type":"end"}
                }
            }})
        };
        let router=Router::new()
            .route(&format!("/api/v2/instances/{instance}"), get(move ||async move{
                Json(json!({"id":instance,"namespace":"sproyt","definition_id":definition,"runtime":"v2","status":"waiting",
                    "active_activations":[{"node_id":"first","status":"waiting","metadata":{"SECRET":true}},{"node_id":"second","status":"waiting"}]}))
            }))
            .route("/api/v2/definitions/sproyt/review/1.0.0",get(move ||async move{Json(make_definition(definition,"1.0.0"))}))
            .route("/api/v2/definitions/sproyt/review/2.0.0",get(move ||async move{Json(make_definition(other,"2.0.0"))}));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let mut row = item("v2", instance);
        row.definition_version = Some("2.0.0".into());
        row.load_definition(&Client::new(), &base).await;
        assert!(row.definition_version.is_none());
        assert!(row.definition_steps.is_none());
        assert!(row.active_node_ids.is_none());
        row.definition_version = Some("1.0.0".into());
        row.load_definition(&Client::new(), &base).await;
        assert_eq!(row.active_node_ids.unwrap(), ["first", "second"]);
        let nodes = row.definition_steps.unwrap();
        assert_eq!(
            nodes.iter().find(|n| n.id == "split").unwrap().next,
            ["first", "second"]
        );
        server.abort();
    }
}
