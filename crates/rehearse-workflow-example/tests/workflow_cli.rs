use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

struct LabTest {
    temporary: PathBuf,
    root: PathBuf,
}

impl LabTest {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let temporary = loop {
            let candidate = std::env::temp_dir().canonicalize().unwrap().join(format!(
                "rehearse-workflow-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&candidate) {
                Ok(()) => break candidate,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("cannot create test directory: {error}"),
            }
        };
        Self {
            root: temporary.join("lab"),
            temporary,
        }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rehearse-workflow-example"));
        command
            .arg(&self.root)
            .args(args)
            .env_remove("GITHUB_ACTIONS")
            .env_remove("GITHUB_STEP_SUMMARY")
            .env_remove("GITHUB_SHA")
            .env_remove("GITHUB_RUN_ID")
            .env_remove("GITHUB_RUN_ATTEMPT")
            .env_remove("GITHUB_JOB")
            // The fixture backend must not require Cargo, Java, Maven, or a shell.
            .env("PATH", "");
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    fn init(&self) {
        assert_success(&self.run(&["--init"]));
    }

    fn gateway_revision(&self, stack: &str) -> String {
        read_json(&self.root.join(format!("gateway/{stack}.json")))["revision"]
            .as_str()
            .unwrap()
            .to_owned()
    }
}

impl Drop for LabTest {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.temporary);
    }
}

fn assert_success(output: &Output) {
    assert!(output.status.success(), "{}", diagnostics(output));
}

fn assert_failure(output: &Output) {
    assert!(!output.status.success(), "{}", diagnostics(output));
}

fn diagnostics(output: &Output) -> String {
    format!(
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn json_output(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("invalid JSON output: {error}\n{}", diagnostics(output)))
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[derive(Debug, PartialEq, Eq)]
enum Entry {
    Directory,
    File(Vec<u8>),
    Symlink(PathBuf),
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Entry> {
    fn visit(root: &Path, directory: &Path, result: &mut BTreeMap<PathBuf, Entry>) {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            let relative = path.strip_prefix(root).unwrap().to_owned();
            let metadata = fs::symlink_metadata(&path).unwrap();
            if metadata.is_symlink() {
                result.insert(relative, Entry::Symlink(fs::read_link(&path).unwrap()));
            } else if metadata.is_dir() {
                result.insert(relative, Entry::Directory);
                visit(root, &path, result);
            } else {
                result.insert(relative, Entry::File(fs::read(path).unwrap()));
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}

fn reports(value: &Value) -> Vec<&Value> {
    let mut result = Vec::new();
    match value {
        Value::Object(object) => {
            if object.contains_key("name") && object.contains_key("outcome") {
                result.push(value);
            }
            for child in object.values() {
                result.extend(reports(child));
            }
        }
        Value::Array(array) => {
            for child in array {
                result.extend(reports(child));
            }
        }
        _ => {}
    }
    result
}

fn outcome<'a>(report: &'a Value, operation: &str) -> &'a Value {
    let nodes = reports(report);
    let node = nodes
        .iter()
        .find(|node| node["name"] == operation)
        .copied()
        .unwrap_or_else(|| panic!("missing {operation} node in {report}"));
    &node["outcome"]
}

fn assert_outcome(report: &Value, operation: &str, kind: &str) {
    let outcome = outcome(report, operation);
    assert!(
        outcome == kind
            || outcome
                .as_object()
                .is_some_and(|fields| fields.contains_key(kind)),
        "expected {operation} to be {kind}, got {outcome}"
    );
}

#[test]
fn static_modes_do_not_need_a_lab_or_invoke_injected_failures() {
    for stack in ["rust", "spring-boot"] {
        for workflow in ["ci", "release", "promote", "audit", "maintenance"] {
            let lab = LabTest::new();
            for mode in ["--describe", "--mermaid"] {
                let output = lab.run(&[
                    "--stack",
                    stack,
                    "--workflow",
                    workflow,
                    mode,
                    "--fail-at",
                    "read-source",
                ]);
                assert_success(&output);
                let text = String::from_utf8(output.stdout).unwrap();
                if mode == "--mermaid" {
                    assert!(text.contains("flowchart TD"));
                    assert!(text.contains("-->"));
                } else {
                    assert!(text.contains(workflow));
                }
                assert!(!lab.root.exists());
            }
            let output = lab.run(&[
                "--stack",
                stack,
                "--workflow",
                workflow,
                "--describe",
                "--format",
                "json",
            ]);
            assert_success(&output);
            let description = json_output(&output);
            assert!(description.to_string().contains("impact"));
            assert!(!lab.root.exists());
        }
    }
}

#[test]
fn json_shorthand_exports_the_canonical_graph_and_preserves_dry_run_reports() {
    let lab = LabTest::new();
    let shorthand = lab.run(&["--describe", "--json", "--fail-at", "read-source"]);
    let explicit = lab.run(&["--describe", "--format", "json"]);
    assert_success(&shorthand);
    assert_success(&explicit);
    let graph = json_output(&shorthand);
    assert_eq!(graph, json_output(&explicit));
    assert_eq!(graph["schema_version"], 1);
    assert_eq!(graph["default_interpretation"], "sequential");
    assert!(graph["nodes"]
        .as_array()
        .is_some_and(|nodes| !nodes.is_empty()));
    assert!(graph["execution"].is_array());
    assert!(!lab.root.exists());

    lab.init();
    let before = snapshot(&lab.root);
    let shorthand = lab.run(&["--json"]);
    let explicit = lab.run(&["--format", "json"]);
    assert_success(&shorthand);
    assert_success(&explicit);
    assert_eq!(json_output(&shorthand), json_output(&explicit));
    assert_eq!(snapshot(&lab.root), before);
}

fn github_node<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["name"] == name)
        .unwrap_or_else(|| panic!("missing {name} in {report}"))
}

#[test]
fn github_reports_preserve_json_and_capture_both_fixture_stacks() {
    for stack in ["rust", "spring-boot"] {
        let lab = LabTest::new();
        lab.init();
        let directory = lab.temporary.join("report");
        let summary = lab.temporary.join("actions-summary.md");
        fs::write(&summary, "Earlier step summary\n").unwrap();
        let output = lab
            .command(&[
                "--stack",
                stack,
                "--execute",
                "--json",
                "--github-report",
                directory.to_str().unwrap(),
            ])
            .env("GITHUB_STEP_SUMMARY", &summary)
            .env("GITHUB_SHA", "fixture-commit")
            .output()
            .unwrap();
        assert_success(&output);
        assert!(json_output(&output).is_object());
        let diagnostics = String::from_utf8_lossy(&output.stderr);
        assert!(diagnostics.contains("::notice"), "{diagnostics}");
        assert!(!String::from_utf8_lossy(&output.stdout).contains("::notice"));
        let described = lab.run(&["--describe", "--json"]);
        assert_success(&described);
        assert_eq!(
            read_json(&directory.join("graph.json")),
            json_output(&described)
        );
        let report = read_json(&directory.join("report.json"));
        assert_eq!(report["outcome"], "succeeded");
        assert_eq!(report["metadata"]["stack"], stack);
        assert_eq!(report["metadata"]["GITHUB_SHA"], "fixture-commit");
        assert_eq!(report["metadata"]["artifact_revision"], "demo-1");
        assert_eq!(
            github_node(&report, "publish_api_gateway")["status"],
            "executed"
        );
        let events = fs::read_to_string(directory.join("events.jsonl")).unwrap();
        assert!(events.lines().count() > 2);
        for event in events.lines() {
            assert!(serde_json::from_str::<Value>(event).unwrap().is_object());
        }
        let markdown = fs::read_to_string(directory.join("summary.md")).unwrap();
        assert!(markdown.contains("publish\\_api\\_gateway"));
        let appended = fs::read_to_string(summary).unwrap();
        assert!(appended.starts_with("Earlier step summary\n"));
        assert!(appended.contains(&markdown));
        assert_eq!(lab.gateway_revision(stack), "demo-1");
    }
}

#[test]
fn github_dry_run_reports_do_not_change_fixtures_and_strict_failure_keeps_node_outcomes() {
    let lab = LabTest::new();
    lab.init();
    let before = snapshot(&lab.root);
    for strict in [false, true] {
        let directory = lab
            .temporary
            .join(if strict { "strict" } else { "rehearsal" });
        let mut args = vec!["--json", "--github-report", directory.to_str().unwrap()];
        if strict {
            args.push("--require-complete");
        }
        let output = lab.run(&args);
        if strict {
            assert_failure(&output);
        } else {
            assert_success(&output);
        }
        let original = json_output(&output);
        assert_outcome(&original, "build", "skipped");
        assert_outcome(&original, "verify_deploy", "blocked");
        let report = read_json(&directory.join("report.json"));
        assert_eq!(
            report["outcome"],
            if strict { "failed" } else { "incomplete" }
        );
        assert_eq!(github_node(&report, "build")["status"], "skipped");
        assert_eq!(github_node(&report, "verify_deploy")["status"], "blocked");
        assert_eq!(
            github_node(&report, "observe_gateway")["status"],
            "executed"
        );
        assert_eq!(snapshot(&lab.root), before);
    }
}

#[test]
fn github_report_records_failed_and_never_started_work() {
    let lab = LabTest::new();
    lab.init();
    let directory = lab.temporary.join("failed-report");
    let output = lab.run(&[
        "--execute",
        "--json",
        "--fail-at",
        "verify",
        "--github-report",
        directory.to_str().unwrap(),
    ]);
    assert_failure(&output);
    assert!(json_output(&output).to_string().contains("injected"));
    let report = read_json(&directory.join("report.json"));
    assert_eq!(report["outcome"], "failed");
    assert_eq!(github_node(&report, "deploy")["status"], "executed");
    assert_eq!(github_node(&report, "verify_deploy")["status"], "failed");
    assert_eq!(
        github_node(&report, "publish_api_gateway")["status"],
        "not_started"
    );
    assert_eq!(
        github_node(&report, "observe_gateway")["status"],
        "not_started"
    );
    assert_eq!(lab.gateway_revision("rust"), "previous");
}

#[test]
fn github_report_paths_and_modes_cannot_modify_fixture_state() {
    let lab = LabTest::new();
    lab.init();
    let before = snapshot(&lab.root);
    for directory in [&lab.root, &lab.root.join("report"), &lab.temporary] {
        let output = lab.run(&["--github-report", directory.to_str().unwrap()]);
        assert_failure(&output);
        assert_eq!(snapshot(&lab.root), before);
    }
    let directory = lab.temporary.join("report");
    for mode in ["--init", "--describe", "--mermaid", "--progress"] {
        assert_failure(&lab.run(&[mode, "--github-report", directory.to_str().unwrap()]));
        assert!(!directory.exists());
        assert_eq!(snapshot(&lab.root), before);
    }
    let output = lab
        .command(&["--github-report", directory.to_str().unwrap()])
        .env("GITHUB_STEP_SUMMARY", lab.root.join("gateway/rust.json"))
        .output()
        .unwrap();
    assert_failure(&output);
    assert!(!directory.exists());
    assert_eq!(snapshot(&lab.root), before);
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("keep.txt"), "unrelated report").unwrap();
    assert_failure(&lab.run(&["--execute", "--github-report", directory.to_str().unwrap()]));
    assert_eq!(snapshot(&lab.root), before);
    assert_eq!(
        fs::read_to_string(directory.join("keep.txt")).unwrap(),
        "unrelated report"
    );
}

#[test]
fn github_finalization_errors_do_not_mask_operation_failures() {
    let lab = LabTest::new();
    lab.init();
    let summary_directory = lab.temporary.join("summary-is-a-directory");
    fs::create_dir(&summary_directory).unwrap();
    let directory = lab.temporary.join("failed-report");
    let output = lab
        .command(&[
            "--execute",
            "--json",
            "--fail-at",
            "verify",
            "--github-report",
            directory.to_str().unwrap(),
        ])
        .env("GITHUB_STEP_SUMMARY", &summary_directory)
        .output()
        .unwrap();
    assert_failure(&output);
    assert!(json_output(&output).to_string().contains("injected"));
    let diagnostics = String::from_utf8_lossy(&output.stderr);
    assert!(
        diagnostics.contains("GitHub reporting also failed"),
        "{diagnostics}"
    );
    assert!(
        diagnostics.contains("Workflow failed: operation"),
        "{diagnostics}"
    );
    assert!(
        diagnostics.contains("injected failure at verify"),
        "{diagnostics}"
    );
    assert_eq!(lab.gateway_revision("rust"), "previous");
}

#[test]
fn github_finalization_failure_returns_nonzero_after_successful_rehearsal() {
    let lab = LabTest::new();
    lab.init();
    let before = snapshot(&lab.root);
    let directory = lab.temporary.join("report");
    let summary_directory = lab.temporary.join("summary-is-a-directory");
    fs::create_dir(&summary_directory).unwrap();
    let output = lab
        .command(&["--json", "--github-report", directory.to_str().unwrap()])
        .env("GITHUB_STEP_SUMMARY", summary_directory)
        .output()
        .unwrap();
    assert_failure(&output);
    assert_outcome(&json_output(&output), "observe_gateway", "executed");
    assert_eq!(
        read_json(&directory.join("report.json"))["outcome"],
        "incomplete"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("Workflow failed:"));
    assert_eq!(snapshot(&lab.root), before);
}

#[cfg(unix)]
#[test]
fn github_report_aliases_cannot_write_inside_the_fixture_lab() {
    use std::os::unix::fs::symlink;

    let lab = LabTest::new();
    lab.init();
    let before = snapshot(&lab.root);
    let alias = lab.temporary.join("fixture-alias");
    symlink(&lab.root, &alias).unwrap();
    let aliased_report = alias.join("report");
    assert_failure(&lab.run(&["--github-report", aliased_report.to_str().unwrap()]));
    let directory = lab.temporary.join("report");
    let output = lab
        .command(&["--github-report", directory.to_str().unwrap()])
        .env("GITHUB_STEP_SUMMARY", alias.join("gateway/rust.json"))
        .output()
        .unwrap();
    assert_failure(&output);
    assert!(!directory.exists());
    assert_eq!(snapshot(&lab.root), before);
}

#[test]
fn all_default_dry_runs_preserve_every_fixture_file() {
    let lab = LabTest::new();
    lab.init();
    let before = snapshot(&lab.root);
    for stack in ["rust", "spring-boot"] {
        for workflow in ["ci", "release", "promote", "audit", "maintenance"] {
            let output = lab.run(&["--stack", stack, "--workflow", workflow]);
            assert_success(&output);
            assert_eq!(snapshot(&lab.root), before, "{stack}/{workflow}");
        }
    }
}

#[test]
fn release_dry_run_checks_policy_before_dependencies_and_continues_independent_reads() {
    let lab = LabTest::new();
    lab.init();
    let output = lab.run(&["--dry-run", "--format", "json"]);
    assert_success(&output);
    let report = json_output(&output);
    for operation in ["build", "test", "deploy", "publish_api_gateway"] {
        assert_outcome(&report, operation, "skipped");
    }
    assert_outcome(&report, "verify_deploy", "blocked");
    assert_outcome(&report, "observe_gateway", "executed");
    assert_eq!(lab.gateway_revision("rust"), "previous");
    assert!(!lab.root.join("artifacts/rust/demo-1.json").exists());
    assert!(!lab.root.join("deployments/rust/demo-1.json").exists());
}

#[test]
fn audit_can_be_complete_while_strict_release_rejects_unavailable_outputs() {
    let lab = LabTest::new();
    lab.init();
    let before = snapshot(&lab.root);
    let audit = lab.run(&[
        "--workflow",
        "audit",
        "--require-complete",
        "--format",
        "json",
    ]);
    assert_success(&audit);
    let audit_report = json_output(&audit);
    let nodes = reports(&audit_report);
    assert!(!nodes.is_empty());
    assert!(nodes.iter().all(|node| node["outcome"] == "executed"));

    let release = lab.run(&["--require-complete", "--format", "json"]);
    assert_failure(&release);
    assert_outcome(&json_output(&release), "verify_deploy", "blocked");
    assert_eq!(snapshot(&lab.root), before);
}

#[test]
fn no_session_policy_denies_authentication_without_stopping_local_observation() {
    let lab = LabTest::new();
    lab.init();
    let before = snapshot(&lab.root);
    let output = lab.run(&[
        "--workflow",
        "audit",
        "--policy",
        "no-session",
        "--format",
        "json",
    ]);
    assert_success(&output);
    let report = json_output(&output);
    assert_outcome(&report, "authenticate", "denied");
    assert_outcome(&report, "read_source", "blocked");
    assert_outcome(&report, "observe_gateway", "executed");
    assert_eq!(snapshot(&lab.root), before);
}

#[test]
fn release_executes_both_stacks_using_only_local_fixtures() {
    let lab = LabTest::new();
    lab.init();
    for stack in ["rust", "spring-boot"] {
        let output = lab.run(&["--stack", stack, "--execute", "--format", "json"]);
        assert_success(&output);
        let summary = json_output(&output);
        assert!(summary.to_string().contains("demo-1"));
        assert_eq!(lab.gateway_revision(stack), "demo-1");
        for folder in ["artifacts", "test-reports", "deployments"] {
            let data = read_json(&lab.root.join(format!("{folder}/{stack}/demo-1.json")));
            assert!(data.is_object());
        }
    }
}

#[test]
fn promotion_reads_a_tested_artifact_and_does_not_build_or_test_it_again() {
    let lab = LabTest::new();
    lab.init();
    let artifacts = snapshot(&lab.root.join("artifacts"));
    let reports = snapshot(&lab.root.join("test-reports"));
    for stack in ["rust", "spring-boot"] {
        let rehearsal = lab.run(&[
            "--stack",
            stack,
            "--workflow",
            "promote",
            "--format",
            "json",
        ]);
        assert_success(&rehearsal);
        assert_outcome(&json_output(&rehearsal), "read_tested_artifact", "executed");
        let execute = lab.run(&["--stack", stack, "--workflow", "promote", "--execute"]);
        assert_success(&execute);
        assert_eq!(lab.gateway_revision(stack), "seed");
        assert!(lab
            .root
            .join(format!("deployments/{stack}/seed.json"))
            .is_file());
    }
    assert_eq!(snapshot(&lab.root.join("artifacts")), artifacts);
    assert_eq!(snapshot(&lab.root.join("test-reports")), reports);
}

#[test]
fn dry_run_failure_is_structured_and_does_not_prevent_independent_work() {
    let lab = LabTest::new();
    lab.init();
    let before = snapshot(&lab.root);
    let output = lab.run(&["--fail-at", "read-source", "--format", "json"]);
    assert_failure(&output);
    let report = json_output(&output);
    assert_outcome(&report, "read_source", "failed");
    assert_outcome(&report, "validate_source", "blocked");
    assert_outcome(&report, "build", "skipped");
    assert_outcome(&report, "observe_gateway", "executed");
    let failure = outcome(&report, "read_source");
    assert_eq!(failure["failed"]["error"]["kind"], "injected");
    assert_eq!(failure["failed"]["error"]["stage"], "read-source");
    assert_eq!(snapshot(&lab.root), before);
}

#[test]
fn failed_verification_stops_execute_and_leaves_the_deployment_without_publishing() {
    let lab = LabTest::new();
    lab.init();
    let gateway = fs::read(lab.root.join("gateway/rust.json")).unwrap();
    let output = lab.run(&["--execute", "--fail-at", "verify", "--progress"]);
    assert_failure(&output);
    assert!(String::from_utf8_lossy(&output.stderr).contains("injected failure at verify"));
    assert!(lab.root.join("deployments/rust/demo-1.json").is_file());
    assert!(lab.root.join("test-reports/rust/demo-1.json").is_file());
    assert_eq!(
        fs::read(lab.root.join("gateway/rust.json")).unwrap(),
        gateway
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("observe_gateway (read) ..."));
}

#[test]
fn maintenance_distinguishes_delete_skip_opaque_deny_and_blocked_values() {
    let lab = LabTest::new();
    lab.init();
    let before = snapshot(&lab.root);
    let output = lab.run(&["--workflow", "maintenance", "--format", "json"]);
    assert_success(&output);
    let report = json_output(&output);
    assert_outcome(&report, "cleanup_stale_deployments", "skipped");
    assert_outcome(&report, "verify_cleanup", "blocked");
    assert_outcome(&report, "legacy_hook", "denied");
    assert_outcome(&report, "inspect_legacy_hook", "blocked");
    assert_outcome(&report, "observe_gateway", "executed");
    assert_eq!(snapshot(&lab.root), before);
}

#[test]
fn maintenance_execution_preserves_the_active_deployment_and_artifacts() {
    let lab = LabTest::new();
    lab.init();
    assert_success(&lab.run(&["--execute"]));
    let artifacts = snapshot(&lab.root.join("artifacts"));
    let gateway = fs::read(lab.root.join("gateway/rust.json")).unwrap();
    assert_success(&lab.run(&["--workflow", "maintenance", "--execute"]));
    assert!(lab.root.join("deployments/rust/demo-1.json").is_file());
    assert!(!lab.root.join("deployments/rust/previous.json").exists());
    assert!(!lab.root.join("deployments/rust/obsolete.json").exists());
    assert!(lab.root.join("legacy/rust.json").is_file());
    assert_eq!(
        fs::read(lab.root.join("gateway/rust.json")).unwrap(),
        gateway
    );
    assert_eq!(snapshot(&lab.root.join("artifacts")), artifacts);
    assert!(lab
        .root
        .join("deployments/spring-boot/previous.json")
        .is_file());
}

#[test]
fn invalid_options_and_reseeding_do_not_change_an_existing_lab() {
    let lab = LabTest::new();
    lab.init();
    let before = snapshot(&lab.root);
    for args in [
        &["--init"][..],
        &["--describe", "--execute"][..],
        &["--dry-run", "--execute"][..],
        &["--init", "--execute"][..],
        &["--execute", "--require-complete"][..],
        &["--execute", "--format", "json", "--progress"][..],
        &["--execute", "--json", "--progress"][..],
        &["--describe", "--json", "--format", "text"][..],
        &["--execute", "--revision", "../escape"][..],
    ] {
        assert_failure(&lab.run(args));
        assert_eq!(snapshot(&lab.root), before, "{args:?}");
    }
    assert!(!lab.temporary.join("escape").exists());
}

#[test]
fn uninitialized_and_unowned_directories_are_never_adopted_implicitly() {
    let lab = LabTest::new();
    assert_failure(&lab.run(&[]));
    assert_failure(&lab.run(&["--execute"]));
    assert!(!lab.root.exists());
    fs::create_dir(&lab.root).unwrap();
    fs::write(lab.root.join("keep.txt"), b"unrelated data").unwrap();
    let before = snapshot(&lab.root);
    assert_failure(&lab.run(&["--init"]));
    assert_failure(&lab.run(&["--execute"]));
    assert_eq!(snapshot(&lab.root), before);
}

#[cfg(unix)]
#[test]
fn symlinks_are_rejected_without_touching_external_targets() {
    use std::os::unix::fs::symlink;
    for relative in [
        "sources/rust/source.rs",
        "deployments/rust",
        "gateway/rust.json",
    ] {
        let lab = LabTest::new();
        lab.init();
        let external = lab.temporary.join("external");
        fs::create_dir(&external).unwrap();
        fs::write(external.join("keep.txt"), b"external content").unwrap();
        let link = lab.root.join(relative);
        let target = if link.is_dir() {
            fs::remove_dir_all(&link).unwrap();
            external.clone()
        } else {
            fs::remove_file(&link).unwrap();
            external.join("keep.txt")
        };
        symlink(target, &link).unwrap();
        for args in [&[][..], &["--execute"][..]] {
            assert_failure(&lab.run(args));
            assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
            assert_eq!(
                fs::read(external.join("keep.txt")).unwrap(),
                b"external content"
            );
            assert_eq!(fs::read_dir(&external).unwrap().count(), 1);
        }
    }
}
