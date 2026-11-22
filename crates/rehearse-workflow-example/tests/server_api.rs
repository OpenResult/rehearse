use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use http_body_util::BodyExt;
use rehearse_workflow_example::server::model::{
    CreateRun, EventKind, NodeSnapshot, NodeStatus, RunMode, RunSnapshot, RunStatus,
};
use rehearse_workflow_example::server::router;
use rehearse_workflow_example::server::state::{ServerConfig, ServerState};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

struct ServerTest {
    directory: PathBuf,
    state: Arc<ServerState>,
    app: axum::Router,
}
impl ServerTest {
    fn new(timeout: Duration, event_capacity: usize, max_runs: usize) -> Self {
        Self::with_delay(timeout, event_capacity, max_runs, Duration::ZERO)
    }
    fn with_delay(
        timeout: Duration,
        event_capacity: usize,
        max_runs: usize,
        step_delay: Duration,
    ) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let directory = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "rehearse-server-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        let state = ServerState::new(ServerConfig {
            root: directory.join("server"),
            approval_timeout: timeout,
            step_delay,
            event_capacity,
            max_runs,
        })
        .unwrap();
        let app = router(state.clone(), "http://127.0.0.1:3000".to_owned());
        Self {
            directory,
            state,
            app,
        }
    }
    fn standard() -> Self {
        Self::new(Duration::from_secs(5), 256, 32)
    }
    async fn request(&self, method: &str, path: &str, body: Option<Value>) -> Response {
        self.app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("host", "127.0.0.1:3000")
                    .header("content-type", "application/json")
                    .body(body.map_or_else(Body::empty, |value| Body::from(value.to_string())))
                    .unwrap(),
            )
            .await
            .unwrap()
    }
    async fn create(&self, stack: &str, mode: &str, fail_at: Option<&str>) -> Value {
        let response = self
            .request(
                "POST",
                "/api/runs",
                Some(json!({"stack":stack,"mode":mode,"fail_at":fail_at})),
            )
            .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        data(response).await
    }
    async fn wait(&self, id: &str, status: RunStatus) -> RunSnapshot {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let snapshot = self.state.get_run(id).unwrap().snapshot();
                if snapshot.status == status {
                    return snapshot;
                }
                assert!(
                    !snapshot.status.is_terminal(),
                    "expected {status:?}, got {snapshot:?}"
                );
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("run reached expected status")
    }
    async fn terminal(&self, id: &str) -> RunSnapshot {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let snapshot = self.state.get_run(id).unwrap().snapshot();
                if snapshot.status.is_terminal() {
                    return snapshot;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("run finished")
    }
    async fn decision(&self, snapshot: &RunSnapshot, choice: &str) -> Response {
        let subject = &snapshot.approval.as_ref().unwrap().subject;
        self.request("POST", &format!("/api/runs/{}/approvals/production", snapshot.id), Some(json!({"decision":choice,"artifact_digest":subject.artifact_digest,"revision":subject.revision}))).await
    }
}
impl Drop for ServerTest {
    fn drop(&mut self) {
        self.state.cancel_all();
        let _ = fs::remove_dir_all(&self.directory);
    }
}
async fn data(response: Response) -> Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}
fn fixture_tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, directory: &Path, entries: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, entries);
            } else {
                entries.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut entries = BTreeMap::new();
    visit(root, root, &mut entries);
    entries
}
fn gateway(snapshot: &RunSnapshot, environment: &str) -> Value {
    serde_json::from_slice(
        &fs::read(
            Path::new(&snapshot.workspace)
                .join(environment)
                .join("gateway")
                .join(format!("{}.json", snapshot.stack)),
        )
        .unwrap(),
    )
    .unwrap()
}
fn has_release_deployment(snapshot: &RunSnapshot, environment: &str) -> bool {
    Path::new(&snapshot.workspace)
        .join(environment)
        .join("deployments")
        .join(snapshot.stack.as_str())
        .join(format!("{}.json", snapshot.revision))
        .exists()
}

fn node<'a>(snapshot: &'a RunSnapshot, name: &str) -> &'a NodeSnapshot {
    let id = snapshot
        .graph
        .nodes()
        .iter()
        .find(|node| node.name() == name)
        .unwrap()
        .id()
        .index();
    snapshot.nodes.iter().find(|node| node.id == id).unwrap()
}

#[tokio::test]
async fn static_plan_and_client_assets_serve_without_creating_runs() {
    let server = ServerTest::standard();
    let before = fixture_tree(&server.directory);
    let plan = data(server.request("GET", "/api/plan", None).await).await;
    assert_eq!(plan["schema_version"], 1);
    assert_eq!(plan["default_interpretation"], "sequential");
    assert_eq!(plan["plan_name"], "production_release");
    assert!(plan["plan_revision"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    assert_eq!(plan["nodes"].as_array().unwrap().len(), 14);
    assert!(plan["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["annotations"]["kind"] == "approval"));
    let group = plan["execution"]
        .as_array()
        .unwrap()
        .iter()
        .find(|step| step["kind"] == "parallel")
        .unwrap();
    assert_eq!(group["group"]["max_concurrency"], 2);
    let branches = group["group"]["branches"].as_array().unwrap();
    assert_eq!(branches.len(), 2);
    assert!(branches
        .iter()
        .all(|branch| branch["nodes"].as_array().unwrap().len() == 2));
    assert_eq!(
        plan,
        serde_json::to_value(rehearse_workflow_example::server::workflow::plan().describe_graph())
            .unwrap()
    );
    assert!(plan.get("approval").is_none());
    assert!(!plan.to_string().contains("Deploy the tested"));
    for path in ["/", "/app.js", "/style.css"] {
        let response = server.request("GET", path, None).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("content-security-policy"));
    }
    assert!(server.state.list_runs().is_empty());
    assert_eq!(fixture_tree(&server.directory), before);
}

#[tokio::test]
async fn dry_run_skips_gate_without_prompting_or_changing_preview_fixtures() {
    let server = ServerTest::standard();
    let before = fixture_tree(&server.directory);
    let created = server.create("rust", "dry-run", None).await;
    let snapshot = server.terminal(created["id"].as_str().unwrap()).await;
    assert_eq!(snapshot.status, RunStatus::Incomplete);
    assert!(snapshot.approval.is_none());
    for name in [
        "build",
        "test",
        "deploy_dev",
        "deploy_staging",
        "request_production_approval",
        "deploy_production",
        "publish_api_gateway",
    ] {
        assert_eq!(node(&snapshot, name).status, NodeStatus::Skipped);
    }
    assert_eq!(
        node(&snapshot, "verify_production").status,
        NodeStatus::Blocked
    );
    assert_eq!(
        node(&snapshot, "observe_gateway").status,
        NodeStatus::Executed
    );
    assert_eq!(fixture_tree(&server.directory), before);
}

#[tokio::test]
async fn parallel_progress_overlaps_and_approval_waits_for_both_verified_environments() {
    let server = ServerTest::with_delay(Duration::from_secs(5), 256, 32, Duration::from_millis(20));
    let created = server.create("rust", "execute", None).await;
    let pending = server
        .wait(created["id"].as_str().unwrap(), RunStatus::AwaitingApproval)
        .await;
    let replay = server.state.get_run(&pending.id).unwrap().events_since(0);
    let ids = [
        node(&pending, "deploy_dev").id,
        node(&pending, "deploy_staging").id,
    ];
    let starts: Vec<_> = replay
        .events
        .iter()
        .filter(|event| {
            matches!(event.kind, EventKind::NodeStarted)
                && event.node_id.is_some_and(|id| ids.contains(&id))
        })
        .collect();
    let finishes: Vec<_> = replay
        .events
        .iter()
        .filter(|event| {
            matches!(event.kind, EventKind::NodeFinished)
                && event.node_id.is_some_and(|id| ids.contains(&id))
        })
        .collect();
    assert_eq!(starts.len(), 2);
    assert_eq!(finishes.len(), 2);
    assert!(starts
        .iter()
        .all(|start| finishes.iter().all(|finish| start.seq < finish.seq)));
    assert!(replay.events.iter().any(|event| event
        .snapshot
        .nodes
        .iter()
        .filter(|node| node.status == NodeStatus::Running)
        .count()
        == 2));
    let gate = replay
        .events
        .iter()
        .find(|event| matches!(event.kind, EventKind::ApprovalRequested))
        .unwrap();
    for name in ["verify_dev", "verify_staging"] {
        let id = node(&pending, name).id;
        let finished = replay
            .events
            .iter()
            .find(|event| {
                matches!(event.kind, EventKind::NodeFinished) && event.node_id == Some(id)
            })
            .unwrap();
        assert!(finished.seq < gate.seq);
        assert_eq!(node(&gate.snapshot, name).status, NodeStatus::Executed);
    }
    assert!(!has_release_deployment(&pending, "production"));
    assert_eq!(
        node(&pending, "deploy_production").status,
        NodeStatus::Pending
    );
    assert!(replay.events.iter().all(|event| event.run_id == pending.id
        && event.plan_revision == pending.plan_revision
        && event.snapshot.graph.plan_revision() == Some(event.plan_revision.as_str())));
    let serialized = serde_json::to_value(&pending).unwrap();
    assert!(serialized["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .all(|node| node.get("name").is_none() && node.get("dependencies").is_none()));
}

#[tokio::test]
async fn failed_parallel_branch_finishes_active_peer_and_never_requests_production() {
    let server = ServerTest::with_delay(Duration::from_secs(5), 256, 32, Duration::from_millis(20));
    let created = server
        .create("spring-boot", "execute", Some("verify-dev"))
        .await;
    let finished = server
        .wait(created["id"].as_str().unwrap(), RunStatus::Failed)
        .await;
    assert_eq!(node(&finished, "verify_dev").status, NodeStatus::Failed);
    assert_eq!(
        node(&finished, "verify_staging").status,
        NodeStatus::Executed
    );
    assert_eq!(
        node(&finished, "request_production_approval").status,
        NodeStatus::NotStarted
    );
    assert_eq!(
        node(&finished, "deploy_production").status,
        NodeStatus::NotStarted
    );
    assert!(finished.approval.is_none());
    assert!(!has_release_deployment(&finished, "production"));
    assert_eq!(gateway(&finished, "production")["revision"], "previous");
    assert!(finished.nodes.iter().all(|node| !matches!(
        node.status,
        NodeStatus::Pending | NodeStatus::Running | NodeStatus::AwaitingApproval
    )));
    let events = server
        .state
        .get_run(&finished.id)
        .unwrap()
        .events_since(0)
        .events;
    let failed_id = node(&finished, "verify_dev").id;
    let peer_id = node(&finished, "verify_staging").id;
    let failed = events
        .iter()
        .find(|event| {
            matches!(event.kind, EventKind::NodeFinished) && event.node_id == Some(failed_id)
        })
        .unwrap();
    let peer_started = events
        .iter()
        .find(|event| {
            matches!(event.kind, EventKind::NodeStarted) && event.node_id == Some(peer_id)
        })
        .unwrap();
    let peer_finished = events
        .iter()
        .find(|event| {
            matches!(event.kind, EventKind::NodeFinished) && event.node_id == Some(peer_id)
        })
        .unwrap();
    assert!(peer_started.seq < failed.seq);
    assert!(failed.seq < peer_finished.seq);
    assert!(!events
        .iter()
        .any(|event| matches!(event.kind, EventKind::ApprovalRequested)));
}

#[tokio::test]
async fn approval_guards_production_and_duplicate_decisions_are_idempotent_for_both_stacks() {
    let server = ServerTest::standard();
    for stack in ["rust", "spring-boot"] {
        let created = server.create(stack, "execute", None).await;
        let pending = server
            .wait(created["id"].as_str().unwrap(), RunStatus::AwaitingApproval)
            .await;
        assert!(has_release_deployment(&pending, "dev"));
        assert!(has_release_deployment(&pending, "staging"));
        assert!(!has_release_deployment(&pending, "production"));
        assert_eq!(gateway(&pending, "production")["revision"], "previous");
        assert!(pending
            .approval
            .as_ref()
            .unwrap()
            .subject
            .artifact_digest
            .starts_with("sha256:"));
        assert_eq!(
            server.decision(&pending, "approve").await.status(),
            StatusCode::OK
        );
        assert_eq!(
            server.decision(&pending, "approve").await.status(),
            StatusCode::OK
        );
        assert_eq!(
            server.decision(&pending, "reject").await.status(),
            StatusCode::CONFLICT
        );
        let completed = server.wait(&pending.id, RunStatus::Succeeded).await;
        assert_eq!(
            gateway(&completed, "production")["revision"],
            completed.revision
        );
        assert!(completed.summary.is_some());
        assert_eq!(
            server.decision(&pending, "approve").await.status(),
            StatusCode::OK
        );
    }
}

#[tokio::test]
async fn rejecting_production_leaves_nonproduction_changes_and_never_publishes() {
    let server = ServerTest::standard();
    let created = server.create("rust", "execute", None).await;
    let pending = server
        .wait(created["id"].as_str().unwrap(), RunStatus::AwaitingApproval)
        .await;
    assert_eq!(
        server.decision(&pending, "reject").await.status(),
        StatusCode::OK
    );
    let finished = server.wait(&pending.id, RunStatus::Rejected).await;
    assert!(has_release_deployment(&finished, "staging"));
    assert!(!has_release_deployment(&finished, "production"));
    assert_eq!(gateway(&finished, "production")["revision"], "previous");
    assert_eq!(
        server.decision(&pending, "reject").await.status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn stale_digest_and_wrong_gate_cannot_resolve_pending_approval() {
    let server = ServerTest::standard();
    let created = server.create("rust", "execute", None).await;
    let pending = server
        .wait(created["id"].as_str().unwrap(), RunStatus::AwaitingApproval)
        .await;
    let response=server.request("POST",&format!("/api/runs/{}/approvals/production",pending.id),Some(json!({"decision":"approve","artifact_digest":"sha256:stale","revision":pending.revision}))).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let response=server.request("POST",&format!("/api/runs/{}/approvals/other",pending.id),Some(json!({"decision":"approve","artifact_digest":pending.approval.as_ref().unwrap().subject.artifact_digest,"revision":pending.revision}))).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        server.state.get_run(&pending.id).unwrap().snapshot().status,
        RunStatus::AwaitingApproval
    );
}

#[tokio::test]
async fn approval_expiry_is_terminal_and_late_approval_is_rejected() {
    let server = ServerTest::new(Duration::from_millis(30), 256, 32);
    let created = server.create("rust", "execute", None).await;
    let finished = server.terminal(created["id"].as_str().unwrap()).await;
    assert_eq!(finished.status, RunStatus::TimedOut);
    assert!(!has_release_deployment(&finished, "production"));
    assert_eq!(
        server.decision(&finished, "approve").await.status(),
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn cancellation_while_waiting_is_terminal_and_repeated_cancel_is_harmless() {
    let server = ServerTest::standard();
    let created = server.create("rust", "execute", None).await;
    let pending = server
        .wait(created["id"].as_str().unwrap(), RunStatus::AwaitingApproval)
        .await;
    let path = format!("/api/runs/{}/cancel", pending.id);
    assert_eq!(
        server
            .request("POST", &path, Some(json!({})))
            .await
            .status(),
        StatusCode::OK
    );
    let finished = server.wait(&pending.id, RunStatus::Cancelled).await;
    assert!(!has_release_deployment(&finished, "production"));
    assert_eq!(
        server
            .request("POST", &path, Some(json!({})))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        server.decision(&pending, "approve").await.status(),
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn failures_before_and_after_approval_stop_later_writes() {
    let server = ServerTest::standard();
    for fail_at in [
        "build",
        "test",
        "verify-dev",
        "verify-staging",
        "verify-production",
    ] {
        let created = server.create("rust", "execute", Some(fail_at)).await;
        let id = created["id"].as_str().unwrap();
        if fail_at == "verify-production" {
            let pending = server.wait(id, RunStatus::AwaitingApproval).await;
            assert_eq!(
                server.decision(&pending, "approve").await.status(),
                StatusCode::OK
            );
        }
        let finished = server.wait(id, RunStatus::Failed).await;
        assert_eq!(gateway(&finished, "production")["revision"], "previous");
        assert_eq!(
            has_release_deployment(&finished, "production"),
            fail_at == "verify-production"
        );
    }
}

#[tokio::test]
async fn changed_artifact_after_question_cannot_be_deployed_under_old_approval() {
    let server = ServerTest::standard();
    let created = server.create("rust", "execute", None).await;
    let pending = server
        .wait(created["id"].as_str().unwrap(), RunStatus::AwaitingApproval)
        .await;
    let artifact = Path::new(&pending.workspace)
        .join("dev/artifacts/rust")
        .join(format!("{}.json", pending.revision));
    let mut source: Value = serde_json::from_slice(&fs::read(&artifact).unwrap()).unwrap();
    source["contents"] = json!("different artifact after approval question");
    fs::write(artifact, serde_json::to_vec_pretty(&source).unwrap()).unwrap();
    assert_eq!(
        server.decision(&pending, "approve").await.status(),
        StatusCode::OK
    );
    let finished = server.wait(&pending.id, RunStatus::Failed).await;
    assert!(!has_release_deployment(&finished, "production"));
    assert_eq!(gateway(&finished, "production")["revision"], "previous");
}

#[tokio::test]
async fn disconnect_does_not_cancel_run_and_reconnect_can_reset_or_replay() {
    let server = ServerTest::new(Duration::from_secs(5), 4, 32);
    let created = server.create("rust", "execute", None).await;
    let id = created["id"].as_str().unwrap();
    let response = server
        .request("GET", &format!("/api/runs/{id}/events"), None)
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    drop(response);
    let pending = server.wait(id, RunStatus::AwaitingApproval).await;
    assert_eq!(
        server.decision(&pending, "approve").await.status(),
        StatusCode::OK
    );
    let finished = server.wait(id, RunStatus::Succeeded).await;
    let response = server
        .request("GET", &format!("/api/runs/{id}/events?after=0"), None)
        .await;
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let stream = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(stream.contains("event: reset"));
    assert!(stream.contains("succeeded"));
    let response = server
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/runs/{id}/events"))
                .header("host", "127.0.0.1:3000")
                .header("last-event-id", (finished.last_event_id - 1).to_string())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8(bytes.to_vec())
        .unwrap()
        .contains("event: progress"));
    let replay = server
        .state
        .get_run(id)
        .unwrap()
        .events_since(finished.last_event_id - 1);
    assert!(replay.reset.is_none());
    assert!(replay
        .events
        .iter()
        .all(|event| event.seq > finished.last_event_id - 1));
}

#[tokio::test]
async fn invalid_requests_foreign_origins_and_capacity_are_reported() {
    let server = ServerTest::new(Duration::from_secs(5), 256, 1);
    let response = server
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/runs")
                .header("host", "127.0.0.1:3000")
                .header("origin", "https://elsewhere.example")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"stack":"rust","mode":"execute"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(server.state.list_runs().is_empty());
    let response = server
        .request("POST", "/api/runs", Some(json!({"stack":"invalid"})))
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let response = server.request("GET", "/api/runs/missing", None).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let first = server.create("rust", "dry-run", None).await;
    server.terminal(first["id"].as_str().unwrap()).await;
    let response = server
        .request("POST", "/api/runs", Some(json!({"stack":"rust"})))
        .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(data(response).await["error"]["code"], "capacity");
}

#[tokio::test]
async fn run_id_and_decision_state_do_not_reappear_after_server_restart() {
    let server = ServerTest::standard();
    let run = server
        .state
        .create_run(CreateRun {
            stack: rehearse_workflow_example::model::Stack::Rust,
            mode: RunMode::DryRun,
            fail_at: None,
        })
        .unwrap();
    server.terminal(&run.id).await;
    let restarted = ServerState::new(ServerConfig {
        root: server.directory.join("server"),
        approval_timeout: Duration::from_secs(5),
        step_delay: Duration::ZERO,
        max_runs: 32,
        event_capacity: 256,
    })
    .unwrap();
    assert!(restarted.list_runs().is_empty());
    assert!(restarted.get_run(&run.id).is_err());
    let next = restarted
        .create_run(CreateRun {
            stack: rehearse_workflow_example::model::Stack::Rust,
            mode: RunMode::DryRun,
            fail_at: None,
        })
        .unwrap();
    assert_ne!(run.id, next.id);
    restarted.cancel_all();
}
