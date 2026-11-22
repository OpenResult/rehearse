#!/usr/bin/env python3
"""Run the built workflow CLI against fresh fixtures and verify GitHub reports.

Python 3.11+ and a built rehearse-workflow-example binary are the only runtime
requirements. Every invocation creates new case directories and retains evidence.
"""

import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import threading


MARKER = ".rehearse-workflow-e2e"
MARKER_CONTENT = "rehearse workflow CLI e2e v1\n"
STACKS = ("rust", "spring-boot")
SCENARIOS = ("dry-run", "release", "failure")


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def checked_path(path):
    path = Path(path).expanduser()
    if ".." in path.parts:
        raise ValueError("parent traversal is not allowed in the E2E root")
    absolute = path.absolute()
    for part in [*reversed(absolute.parents), absolute]:
        if part.is_symlink():
            raise ValueError(f"symlinks are not allowed in the E2E root: {part}")
    return absolute


def prepare_root(path):
    root = checked_path(path)
    checkout = Path(__file__).resolve().parent.parent
    if root == Path.home().resolve() or root == Path(root.anchor) or root in checkout.parents or root == checkout:
        raise ValueError("choose a dedicated E2E output directory")
    if root.exists():
        if not root.is_dir():
            raise ValueError("the E2E root is not a directory")
        marker = root / MARKER
        if marker.is_symlink():
            raise ValueError("the E2E ownership marker must not be a symlink")
        if marker.exists():
            if marker.read_text() != MARKER_CONTENT:
                raise ValueError("the E2E ownership marker belongs to another application")
        elif any(root.iterdir()):
            raise ValueError("refusing a nonempty E2E root without an ownership marker")
    else:
        root.mkdir(parents=True)
    marker = root / MARKER
    if not marker.exists():
        with marker.open("x") as stream:
            stream.write(MARKER_CONTENT)
    return root


def snapshot(root):
    entries = {}
    for path in sorted(root.rglob("*")):
        mode = path.lstat().st_mode
        if stat.S_ISDIR(mode):
            entry = ("directory", b"")
        elif stat.S_ISREG(mode):
            entry = ("file", path.read_bytes())
        else:
            raise AssertionError(f"unexpected fixture filesystem entry: {path}")
        entries[str(path.relative_to(root))] = entry
    return entries


def read_json(path):
    with path.open() as stream:
        return json.load(stream)


def invoke(binary, lab, args, log, live=False):
    env = os.environ.copy()
    # Prove that fixture operations do not depend on build tools or a shell.
    env["PATH"] = ""
    command = [str(binary), str(lab), *args]
    if not live:
        result = subprocess.run(command, env=env, capture_output=True, text=True, timeout=30)
        log.write_text(result.stdout + result.stderr)
        return result.returncode, result.stdout

    with log.open("w") as transcript:
        process = subprocess.Popen(command, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                   text=True, errors="replace", bufsize=1)

        def forward():
            for line in process.stdout:
                transcript.write(line)
                transcript.flush()
                print(line, end="", flush=True)

        reader = threading.Thread(target=forward, daemon=True)
        reader.start()
        try:
            code = process.wait(timeout=30)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
            reader.join()
            raise
        reader.join()
        process.stdout.close()
    return code, ""


def validate_reports(directory, graph, mode, outcome, metadata=None):
    require(read_json(directory / "graph.json") == graph, "reported graph differs from static Describe")
    report = read_json(directory / "report.json")
    require(report["schema_version"] == 1, "unsupported report schema")
    require(report["plan_name"] == graph["plan_name"], "report belongs to another plan")
    require(report.get("plan_revision") == graph.get("plan_revision"), "report revision differs from graph")
    require(report["mode"] == mode, f"expected {mode} report")
    require(report["outcome"] == outcome, f"expected {outcome}, got {report['outcome']}")
    for key, expected in (metadata or {}).items():
        require(report["metadata"].get(key) == expected, f"report metadata {key} differs from the case")
    ids = [node["id"] for node in graph["nodes"]]
    require([node["id"] for node in report["nodes"]] == ids, "report rows are not in declaration order")
    for row, node in zip(report["nodes"], graph["nodes"]):
        require(row["name"] == node["name"], "report node metadata differs from graph")
        if row["status"] == "not_started":
            require(row["duration_ms"] is None, "unstarted nodes cannot have execution durations")
        else:
            require(isinstance(row["duration_ms"], int) and row["duration_ms"] >= 0,
                    "started nodes must retain execution timing")
    events = [json.loads(line) for line in (directory / "events.jsonl").read_text().splitlines()]
    require(bool(events), "no progress events retained")
    require([event["sequence"] for event in events] == list(range(1, len(events) + 1)),
            "event sequence has gaps or duplicates")
    require(events[0]["kind"] == "plan_started" and events[-1]["kind"] == "plan_finished",
            "missing plan lifecycle events")
    for event in events:
        require(event["schema_version"] == 1, "unsupported event schema")
        require(event["plan_name"] == graph["plan_name"], "event belongs to another plan")
        require(event.get("plan_revision") == graph.get("plan_revision"), "event revision differs from graph")
        require(event["mode"] == mode, "event interpretation differs from the run")
        require(event["node_id"] is None or event["node_id"] in ids, "event references an unknown node")
    require(bool((directory / "summary.md").read_text().strip()), "empty Markdown summary")
    return {node["name"]: node["status"] for node in report["nodes"]}


def run_case(binary, directory, stack, scenario):
    directory.mkdir(parents=True)
    lab = directory / "lab"
    reports = directory / "reports"
    revision = f"e2e-{stack}-{scenario}"
    common = ["--stack", stack, "--workflow", "release", "--revision", revision]
    code, text = invoke(binary, lab, [*common, "--describe", "--json"], directory / "describe.log")
    require(code == 0, "static Describe failed")
    graph = json.loads(text)
    require(graph["schema_version"] == 1 and graph["plan_name"] == "release", "unexpected static graph")
    require(not lab.exists(), "Describe accessed or created the lab")
    (directory / "describe.json").write_text(json.dumps(graph, indent=2) + "\n")
    code, _ = invoke(binary, lab, ["--init"], directory / "init.log")
    require(code == 0, "fixture initialization failed")
    before = snapshot(lab)
    gateway_path = lab / "gateway" / f"{stack}.json"
    previous_gateway = gateway_path.read_bytes()
    args = [*common, "--github-report", str(reports)]
    if scenario != "dry-run":
        args.append("--execute")
    if scenario == "failure":
        args.extend(["--fail-at", "verify"])
        print("Expecting an injected verification failure; publication must remain unstarted.", flush=True)
    code, _ = invoke(binary, lab, args, directory / "run.log", live=True)
    mode = "dry_run" if scenario == "dry-run" else "execute"
    outcome = {"dry-run": "incomplete", "release": "succeeded", "failure": "failed"}[scenario]
    statuses = validate_reports(reports, graph, mode, outcome,
                                {"stack": stack, "workflow": "release", "artifact_revision": revision})
    if scenario == "dry-run":
        require(code == 0, "safe dry-run failed")
        require(snapshot(lab) == before, "dry-run changed fixture files")
        for name in ("build", "test", "deploy", "publish_api_gateway"):
            require(statuses[name] == "skipped", f"{name} was not skipped by policy")
        require(statuses["verify_deploy"] == "blocked", "receipt-dependent verification was not blocked")
        require(statuses["observe_gateway"] == "executed", "independent gateway observation did not run")
    elif scenario == "release":
        require(code == 0, "fixture release failed")
        require(all(status == "executed" for status in statuses.values()), "release left unfinished operations")
        require(read_json(gateway_path)["revision"] == revision, "gateway did not publish the release")
        for area in ("artifacts", "test-reports", "deployments"):
            require((lab / area / stack / f"{revision}.json").is_file(), f"missing {area} evidence")
    else:
        require(code != 0, "injected verification failure unexpectedly succeeded")
        require(statuses["verify_deploy"] == "failed", "failure was not attributed to verification")
        for name in ("publish_api_gateway", "observe_gateway", "summarize_release"):
            require(statuses[name] == "not_started", f"{name} started after execute failure")
        require(gateway_path.read_bytes() == previous_gateway, "failed verification published a gateway route")
        require((lab / "deployments" / stack / f"{revision}.json").is_file(),
                "completed deployment evidence disappeared after failure")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/debug/rehearse-workflow-example"))
    parser.add_argument("--root", type=Path, default=Path("target/workflow-e2e"))
    parser.add_argument("--stack", choices=("all", *STACKS), default="all")
    parser.add_argument("--scenario", choices=("suite", *SCENARIOS), default="suite")
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    require(binary.is_file(), "the CLI binary must be built before running E2E")
    root = prepare_root(args.root)
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    run = Path(tempfile.mkdtemp(prefix=f"run-{stamp}-", dir=root))
    print(f"[workflow-e2e] retained evidence: {run}", flush=True)
    results = []
    stacks = STACKS if args.stack == "all" else (args.stack,)
    scenarios = SCENARIOS if args.scenario == "suite" else (args.scenario,)
    for stack in stacks:
        for scenario in scenarios:
            print(f"[workflow-e2e] checking {stack}/{scenario}", flush=True)
            try:
                run_case(binary, run / stack / scenario, stack, scenario)
                result = {"stack": stack, "scenario": scenario, "status": "passed"}
            except (AssertionError, KeyError, ValueError, OSError, subprocess.SubprocessError) as error:
                result = {"stack": stack, "scenario": scenario, "status": "failed", "error": str(error)}
            results.append(result)
            print(f"[workflow-e2e] {stack}/{scenario}: {result['status']}", flush=True)
            if "error" in result:
                print(result["error"], file=sys.stderr)
    (run / "e2e.json").write_text(json.dumps({"schema_version": 1, "cases": results}, indent=2) + "\n")
    lines = ["## Workflow example E2E", "", "Expected injected failures pass when publication is prevented.",
             "", "| Stack | Scenario | E2E result |", "| --- | --- | --- |"]
    lines.extend(f"| {case['stack']} | {case['scenario']} | {case['status']} |" for case in results)
    summary = "\n".join(lines) + "\n"
    (run / "e2e-summary.md").write_text(summary)
    if path := os.environ.get("GITHUB_STEP_SUMMARY"):
        with Path(path).open("a") as stream:
            stream.write("\n" + summary)
    return 0 if all(case["status"] == "passed" for case in results) else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (AssertionError, OSError, ValueError) as error:
        print(f"[workflow-e2e] {error}", file=sys.stderr)
        sys.exit(1)
