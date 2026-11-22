(() => {
  "use strict";

  const $ = (id) => document.getElementById(id);
  const terminal = new Set(["succeeded", "incomplete", "rejected", "timed_out", "cancelled", "failed"]);
  const state = {
    plan: null,
    runs: new Map(),
    runButtons: new Map(),
    selected: null,
    source: null,
    cursor: 0,
    manualDisconnect: false,
    loadingRun: false,
    submitting: false,
    deciding: false,
    cancelling: false,
    errorKind: null,
    log: [],
    announcedStatus: "",
  };

  const statusLabels = {
    pending: "Not started", not_started: "Not run", queued: "Queued", running: "Running",
    awaiting_approval: "Awaiting approval", succeeded: "Succeeded", executed: "Executed",
    incomplete: "Incomplete", skipped: "Skipped", denied: "Denied", blocked: "Blocked",
    failed: "Failed", rejected: "Rejected", approved: "Approved", timed_out: "Timed out",
    cancelled: "Cancelled",
  };
  const phaseDefinitions = [
    { key: "build", name: "Build & test" },
    { key: "dev", name: "Dev" },
    { key: "staging", name: "Staging" },
    { key: "approval", name: "Approval" },
    { key: "production", name: "Production" },
    { key: "gateway", name: "Gateway" },
  ];

  function element(tag, className, text) {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text !== undefined) node.textContent = text;
    return node;
  }

  function label(value) {
    return statusLabels[value] || String(value || "").replaceAll("_", " ").replaceAll("-", " ");
  }

  function stackName(stack) {
    return stack === "spring-boot" ? "Spring Boot" : "Rust";
  }

  function time(ms, includeDate = false) {
    if (!Number.isFinite(Number(ms))) return "—";
    const options = includeDate
      ? { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", second: "2-digit" }
      : { hour: "2-digit", minute: "2-digit", second: "2-digit", hour12: false };
    return new Intl.DateTimeFormat(undefined, options).format(new Date(Number(ms)));
  }

  function statusIcon(status) {
    const ns = "http://www.w3.org/2000/svg";
    const svg = document.createElementNS(ns, "svg");
    svg.setAttribute("viewBox", "0 0 24 24");
    svg.setAttribute("aria-hidden", "true");
    const path = document.createElementNS(ns, "path");
    const paths = {
      executed: "m5 12 4 4L19 6", succeeded: "m5 12 4 4L19 6", approved: "m5 12 4 4L19 6",
      running: "M12 3a9 9 0 0 1 9 9M12 21a9 9 0 0 1-9-9",
      awaiting_approval: "M9 5v14M15 5v14", pending: "M8 12h8",
      failed: "m7 7 10 10M17 7 7 17", rejected: "m7 7 10 10M17 7 7 17",
      cancelled: "m7 7 10 10M17 7 7 17", skipped: "m7 5 9 7-9 7V5Zm11 0v14",
      denied: "M5 5 19 19M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18Z",
      blocked: "M7 10V7a5 5 0 0 1 10 0v3M6 10h12v11H6V10Z",
      timed_out: "M12 7v5l3 2M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18Z",
    };
    path.setAttribute("d", paths[status] || paths.pending);
    svg.append(path);
    return svg;
  }

  function badge(status, text) {
    const span = element("span", "status-badge", text || label(status));
    span.dataset.status = status;
    return span;
  }

  function showError(message, kind = "request") {
    state.errorKind = kind;
    $("error-message").textContent = message;
    $("error-banner").hidden = false;
  }

  function clearError(kind) {
    if (kind && state.errorKind !== kind) return;
    state.errorKind = null;
    $("error-banner").hidden = true;
    $("error-message").textContent = "";
  }

  async function api(path, options = {}) {
    let response;
    try {
      response = await fetch(path, {
        cache: "no-store",
        ...options,
        headers: options.body ? { "Content-Type": "application/json", ...options.headers } : options.headers,
      });
    } catch (_) {
      const error = new Error("Cannot reach the local server. Check that it is running, then refresh the run list or reconnect.");
      error.network = true;
      throw error;
    }
    let data;
    try {
      data = await response.json();
    } catch (_) {
      const error = new Error(`The local server returned an unexpected response (${response.status}).`);
      error.status = response.status;
      throw error;
    }
    if (!response.ok) {
      const error = new Error(data.error?.message || `Request failed (${response.status}).`);
      error.status = response.status;
      error.code = data.error?.code;
      throw error;
    }
    return data;
  }

  function setConnection(kind, text) {
    $("connection-status").dataset.state = kind;
    $("connection-label").textContent = text;
    const hasRun = Boolean(state.selected);
    const hasNewEvents = hasRun && state.cursor < Number(state.selected.last_event_id || 0);
    $("toggle-stream").disabled = !hasRun || (terminal.has(state.selected.status) && !hasNewEvents);
    $("toggle-stream").textContent = state.manualDisconnect ? "Reconnect live view" : "Disconnect live view";
  }

  function closeSource() {
    if (state.source) state.source.close();
    state.source = null;
  }

  function runPath(runId) {
    return `/api/runs/${encodeURIComponent(runId)}`;
  }

  function rememberRun(id) {
    const url = new URL(window.location.href);
    if (id) url.searchParams.set("run", id);
    else url.searchParams.delete("run");
    window.history.replaceState(null, "", url);
  }

  function updateRunButton(run) {
    let parts = state.runButtons.get(run.id);
    if (!parts) {
      const li = element("li");
      const button = element("button");
      button.type = "button";
      button.dataset.runId = run.id;
      const icon = element("span", "stack-icon");
      icon.setAttribute("aria-hidden", "true");
      const copy = element("span", "run-list-copy");
      const name = element("span", "run-list-name");
      const meta = element("span", "run-list-meta");
      const status = element("span", "run-list-state");
      const timestamp = element("span", "run-list-time");
      meta.append(status, timestamp);
      copy.append(name, meta);
      button.append(icon, copy);
      button.addEventListener("click", () => selectRun(run.id));
      li.append(button);
      parts = { li, button, icon, name, status, timestamp };
      state.runButtons.set(run.id, parts);
    }
    parts.icon.textContent = run.stack === "spring-boot" ? "J" : "R";
    parts.name.textContent = `${stackName(run.stack)} · ${run.revision}`;
    parts.status.textContent = label(run.status);
    parts.status.dataset.status = run.status;
    parts.timestamp.textContent = time(run.created_at_ms).slice(0, 5);
    parts.button.setAttribute("aria-current", String(state.selected?.id === run.id));
    parts.button.setAttribute("aria-label", `${stackName(run.stack)}, ${run.revision}, ${label(run.status)}`);
    parts.button.title = run.id;
    return parts;
  }

  function renderRuns() {
    const sorted = [...state.runs.values()].sort((a, b) => b.created_at_ms - a.created_at_ms);
    const present = new Set(sorted.map((run) => run.id));
    for (const [id, parts] of state.runButtons) {
      if (!present.has(id)) {
        parts.li.remove();
        state.runButtons.delete(id);
      }
    }
    sorted.forEach((run, index) => {
      const parts = updateRunButton(run);
      const currentAtIndex = $("run-list").children[index];
      if (currentAtIndex !== parts.li) $("run-list").insertBefore(parts.li, currentAtIndex || null);
    });
    $("run-count").textContent = String(sorted.length);
    $("runs-empty").hidden = sorted.length > 0;
  }

  function phaseFor(node) {
    if (node.kind === "approval") return "approval";
    if (["dev", "staging", "production", "gateway"].includes(node.environment)) return node.environment;
    if (node.environment === "summary") return null;
    return "build";
  }

  function currentGraph() {
    return state.selected?.graph || state.plan;
  }

  function viewNodes() {
    const graph = currentGraph();
    const outcomes = new Map((state.selected?.nodes || []).map((node) => [Number(node.id), node]));
    return (graph?.nodes || []).map((node) => ({
      ...node,
      ...outcomes.get(Number(node.id)),
      label: node.annotations?.label || node.name,
      kind: node.annotations?.kind || "operation",
      environment: node.annotations?.environment || null,
    }));
  }

  function phaseStatus(nodes) {
    const statuses = nodes.map((node) => node.status || "pending");
    if (statuses.includes("failed")) return "failed";
    if (statuses.includes("awaiting_approval")) return "awaiting_approval";
    if (statuses.includes("running")) return "running";
    if (statuses.length && statuses.every((status) => status === "executed")) return "executed";
    if (statuses.includes("cancelled")) return "cancelled";
    if (statuses.includes("pending")) return "pending";
    if (statuses.includes("denied")) return "denied";
    if (statuses.includes("blocked")) return "blocked";
    if (statuses.includes("skipped")) return "skipped";
    if (statuses.includes("not_started")) return "not_started";
    return "pending";
  }

  function renderPhases(nodes) {
    const phases = phaseDefinitions.map((phase) => {
      const matching = nodes.filter((node) => phaseFor(node) === phase.key);
      let status = phaseStatus(matching);
      if (phase.key === "approval" && state.selected?.approval) {
        const approvalStatus = state.selected.approval.status;
        status = approvalStatus === "pending" ? "awaiting_approval" : approvalStatus === "approved" ? "executed" : approvalStatus;
      }
      const li = element("li", "phase-card");
      li.dataset.status = status;
      const symbol = element("span", "phase-symbol");
      symbol.append(statusIcon(status));
      const copy = element("span", "phase-copy");
      copy.append(element("span", "phase-name", phase.name));
      let caption = label(status);
      if (status === "pending" && state.selected && terminal.has(state.selected.status)) caption = "Not run";
      copy.append(element("span", "phase-caption", caption));
      li.append(symbol, copy);
      return li;
    });
    $("phase-strip").replaceChildren(...phases);
  }

  function renderNode(node) {
    const status = node.status || "pending";
    const row = element("li", "node");
    row.dataset.status = status;
    row.dataset.nodeId = String(node.id);
    const symbol = element("span", "node-icon");
    symbol.append(statusIcon(status));
    const copy = element("div", "node-copy");
    copy.append(element("span", "node-name", node.label || node.name));
    const operation = element("span", "node-operation", node.name);
    operation.title = node.dependencies?.length ? `Value inputs from node${node.dependencies.length > 1 ? "s" : ""} ${node.dependencies.join(", ")}` : "No value dependencies";
    copy.append(operation);
    if (node.dependencies?.length) copy.append(element("span", "value-inputs", `Inputs: ${node.dependencies.join(", ")}`));
    if (node.message) copy.append(element("p", "node-message", node.message));
    const metadata = element("div", "node-metadata");
    const impact = element("span", "impact-tag", node.impact);
    impact.dataset.impact = node.impact;
    const stateText = status === "pending" && state.selected && terminal.has(state.selected.status) ? "Not run" : label(status);
    metadata.append(impact, badge(status, stateText));
    row.append(symbol, copy, metadata);
    return row;
  }

  function renderNodes(nodes) {
    const graph = currentGraph();
    const byId = new Map(nodes.map((node) => [Number(node.id), node]));
    const fragment = document.createDocumentFragment();
    const steps = graph?.execution || nodes.map((node) => ({kind: "node", node: node.id}));
    for (const step of steps) {
      if (step.kind === "node") {
        const node = byId.get(Number(step.node));
        if (node) fragment.append(renderNode(node));
        continue;
      }
      if (step.kind !== "parallel") continue;
      const group = step.group;
      const region = element("li", "parallel-region");
      region.dataset.groupId = String(group.id);
      region.setAttribute("aria-label", `${group.name}: parallel region, concurrency limit ${group.max_concurrency}`);
      const heading = element("div", "parallel-heading");
      const title = element("div");
      title.append(element("h3", "parallel-name", group.name), element("p", "parallel-explanation", "Branches may overlap; operations within each branch stay ordered."));
      const running = group.branches.flatMap((branch) => branch.nodes).filter((id) => ["running", "awaiting_approval"].includes(byId.get(Number(id))?.status)).length;
      heading.append(title, element("span", "parallel-limit", `${running} active · limit ${group.max_concurrency}`));
      const lanes = element("div", "parallel-lanes");
      for (const branch of group.branches) {
        const lane = element("section", "parallel-lane");
        const laneId = `branch-${group.id}-${group.branches.indexOf(branch)}`;
        const labelNode = element("h4", "branch-name", branch.name);
        labelNode.id = laneId;
        lane.setAttribute("aria-labelledby", laneId);
        const list = element("ol", "branch-nodes");
        for (const id of branch.nodes) {
          const node = byId.get(Number(id));
          if (node) list.append(renderNode(node));
        }
        lane.append(labelNode, list);
        lanes.append(lane);
      }
      const joinText = state.selected?.mode === "dry-run"
        ? "Join barrier · every branch is assessed before later operations; unavailable values still block their consumers."
        : "Join barrier · execution continues only after every branch succeeds.";
      region.append(heading, lanes, element("p", "parallel-join", joinText));
      fragment.append(region);
    }
    $("node-list").replaceChildren(fragment);
  }

  function renderApproval() {
    const run = state.selected;
    const approval = run?.approval;
    const card = $("approval-card");
    $("approval-details").hidden = !approval;
    $("approval-buttons").hidden = !approval || approval.status !== "pending";
    if (!approval) {
      card.dataset.status = "idle";
      $("approval-status").dataset.status = "pending";
      if (run?.mode === "dry-run") {
        $("approval-status").textContent = "Skipped";
        $("approval-question").textContent = "Dry-run skips the interactive gate. It creates no production permit and requests no decision.";
      } else if (run && terminal.has(run.status)) {
        $("approval-status").textContent = "Not reached";
        $("approval-question").textContent = "This run ended before requesting production approval.";
      } else {
        $("approval-status").textContent = "Decision gate";
        $("approval-question").textContent = "Production waits until dev and staging have been deployed and verified.";
      }
      $("approval-note").textContent = "The decision is bound to this run, revision, and artifact. No production permit is created by a dry-run.";
      return;
    }
    card.dataset.status = approval.status;
    $("approval-status").dataset.status = approval.status === "pending" ? "awaiting_approval" : approval.status;
    $("approval-status").textContent = approval.status === "pending" ? "Waiting for you" : label(approval.status);
    $("approval-target").textContent = label(approval.subject.environment);
    $("approval-revision").textContent = approval.subject.revision;
    $("approval-digest").textContent = approval.subject.artifact_digest;
    const descriptions = {
      approved: "Approved. The release can continue with this exact artifact and revision.",
      rejected: "Release rejected. Production deployment and gateway publication did not run.",
      timed_out: "The decision window expired. Production deployment and gateway publication did not run.",
      cancelled: "Run cancelled. Fixture changes completed before cancellation remain in place.",
    };
    $("approval-question").textContent = descriptions[approval.status] || approval.question;
    $("approval-note").textContent = approval.status === "pending"
      ? "Approve continues this run into production. Reject stops it. This local example uses a single local operator."
      : "This decision applies only to this run, artifact digest, revision, and target environment.";
    updateCountdown();
  }

  function updateCountdown() {
    const approval = state.selected?.approval;
    if (!approval) return;
    const pending = approval.status === "pending" && !terminal.has(state.selected.status);
    const remainingMs = Number(approval.expires_at_ms) - Date.now();
    const seconds = Math.max(0, Math.ceil(remainingMs / 1000));
    if (pending) {
      $("approval-countdown").textContent = seconds > 0
        ? `Decision expires in ${Math.floor(seconds / 60)}m ${String(seconds % 60).padStart(2, "0")}s`
        : "Decision window expired; waiting for server status.";
    } else {
      $("approval-countdown").textContent = `Requested ${time(approval.requested_at_ms)} · deadline ${time(approval.expires_at_ms)}`;
    }
    const disabled = !pending || remainingMs <= 0 || state.deciding || state.cancelling;
    $("approve-run").disabled = disabled;
    $("reject-run").disabled = disabled;
  }

  function renderCurrent() {
    const run = state.selected;
    const nodes = viewNodes();
    renderPhases(nodes);
    renderNodes(nodes);
    renderApproval();
    $("plan-kind").textContent = `Describe JSON v${currentGraph()?.schema_version || 1}`;
    $("timeline-description").textContent = run?.mode === "dry-run"
      ? "Policy is checked before value dependencies. Skipped and blocked remain distinct."
      : "Declared control order and parallel regions; input links are separate value dependencies.";
    $("cancel-run").hidden = !run || terminal.has(run.status);
    $("cancel-run").disabled = state.cancelling;
    $("cancel-run").textContent = state.cancelling ? "Cancelling…" : "Cancel run";
    if (!run) {
      $("run-title").textContent = "Release plan";
      $("run-status").dataset.status = "pending";
      $("run-status").textContent = "Ready to start";
      $("run-subtitle").textContent = "Preview the workflow and its parallel branches, or select an existing run.";
      $("step-count").textContent = nodes.length ? `${nodes.length} operations` : "";
      for (const name of ["id", "stack", "mode", "created"]) $(`detail-${name}`).textContent = "—";
      $("detail-workspace").textContent = "Each executed run gets its own fixture files.";
      $("detail-plan-revision").textContent = state.plan?.plan_revision || "—";
      $("result-card").hidden = true;
      return;
    }
    $("run-title").textContent = `${stackName(run.stack)} release`;
    $("run-status").dataset.status = run.status;
    $("run-status").textContent = label(run.status);
    $("run-subtitle").textContent = `${run.revision} · ${run.mode === "dry-run" ? "Safe dry-run" : "Execute fixtures"}`;
    const completed = nodes.filter((node) => node.status === "executed").length;
    $("step-count").textContent = `${completed} / ${nodes.length} executed`;
    $("detail-id").textContent = run.id;
    $("detail-plan-revision").textContent = run.plan_revision;
    $("detail-stack").textContent = stackName(run.stack);
    $("detail-mode").textContent = run.mode === "dry-run" ? "Safe dry-run" : "Execute local fixtures";
    $("detail-created").textContent = time(run.created_at_ms, true);
    $("detail-workspace").textContent = run.workspace;
    const resultText = run.summary?.message || run.error?.message || {
      incomplete: "Dry-run finished with skipped or blocked work. No production approval was requested.",
      rejected: "The production decision was rejected. Earlier fixture deployments remain in place.",
      timed_out: "Production approval timed out. Earlier fixture deployments remain in place.",
      cancelled: "Run cancelled. Fixture writes completed before cancellation are retained.",
      failed: "The run stopped admitting new work after an operation failed. Active operations finished before the run ended.",
    }[run.status];
    $("result-card").hidden = !terminal.has(run.status) || !resultText;
    $("result-card").dataset.status = run.status;
    $("result-message").textContent = resultText || "";
    if (state.announcedStatus !== `${run.id}:${run.status}`) {
      $("status-announcement").textContent = `${stackName(run.stack)} release: ${label(run.status)}.`;
      state.announcedStatus = `${run.id}:${run.status}`;
    }
  }

  function applySnapshot(snapshot) {
    if (!snapshot?.id) return;
    if (!snapshot.graph || snapshot.graph.plan_revision !== snapshot.plan_revision) {
      showError("Run snapshot does not match its described plan revision.");
      return;
    }
    const previous = state.runs.get(snapshot.id);
    if (!previous || Number(snapshot.last_event_id) >= Number(previous.last_event_id)) state.runs.set(snapshot.id, snapshot);
    if (!state.selected || state.selected.id !== snapshot.id) {
      renderRuns();
      return;
    }
    if (Number(snapshot.last_event_id) < Number(state.selected.last_event_id)) return;
    state.selected = snapshot;
    renderRuns();
    renderCurrent();
    if (terminal.has(snapshot.status) && state.cursor >= Number(snapshot.last_event_id)) {
      closeSource();
      setConnection("idle", "Run finished");
    }
  }

  function clearActivity() {
    state.log = [];
    $("activity-list").replaceChildren();
    $("activity-empty").hidden = false;
    $("activity-empty").textContent = "Connecting to the run event stream…";
    $("event-count").textContent = "0 events";
  }

  function appendEvent(event, reset = false) {
    if (!reset && Number(event.seq) <= state.cursor) return;
    if (reset) {
      state.log = [];
      $("activity-list").replaceChildren();
    }
    state.cursor = Number(event.seq);
    const nearBottom = $("activity-list").scrollHeight - $("activity-list").scrollTop - $("activity-list").clientHeight < 55;
    const row = element("li", "activity-item");
    row.dataset.kind = event.kind;
    const timestamp = element("time", "activity-time", time(event.at_ms));
    timestamp.dateTime = new Date(Number(event.at_ms)).toISOString();
    const message = element("span", "activity-message", reset ? "History refreshed to the latest server snapshot." : event.message);
    message.title = `Event ${event.seq} · ${label(event.kind)}`;
    row.append(timestamp, message);
    $("activity-list").append(row);
    state.log.push(event.seq);
    while (state.log.length > 100) {
      state.log.shift();
      $("activity-list").firstElementChild?.remove();
    }
    $("activity-empty").hidden = true;
    $("event-count").textContent = `${state.log.length} event${state.log.length === 1 ? "" : "s"}`;
    if (nearBottom) $("activity-list").scrollTop = $("activity-list").scrollHeight;
  }

  function connectStream() {
    closeSource();
    const run = state.selected;
    if (!run || state.manualDisconnect) return;
    if (terminal.has(run.status) && state.cursor >= Number(run.last_event_id)) {
      setConnection("idle", "Run finished");
      return;
    }
    const runId = run.id;
    const source = new EventSource(`${runPath(runId)}/events?after=${state.cursor}`);
    state.source = source;
    setConnection("reconnecting", "Connecting live view");
    source.onopen = () => {
      if (state.source !== source) return;
      clearError("connection");
      setConnection("live", "Live event stream");
    };
    const receive = (event, reset) => {
      if (state.source !== source || state.selected?.id !== runId) return;
      try {
        const payload = JSON.parse(event.data);
        if (payload.run_id !== runId || payload.plan_revision !== state.selected.plan_revision) return;
        appendEvent(payload, reset);
        applySnapshot(payload.snapshot);
        if (terminal.has(state.selected.status) && state.cursor >= Number(state.selected.last_event_id)) {
          closeSource();
          setConnection("idle", "Run finished");
        }
      } catch (_) {
        showError("The event stream returned an unexpected message. Reconnect to refresh the run snapshot.");
      }
    };
    source.addEventListener("progress", (event) => receive(event, false));
    source.addEventListener("reset", (event) => receive(event, true));
    source.onerror = async () => {
      if (state.source !== source || state.manualDisconnect) return;
      setConnection("reconnecting", "Reconnecting live view");
      try {
        const snapshot = await api(runPath(runId));
        if (state.selected?.id !== runId) return;
        applySnapshot(snapshot);
      } catch (error) {
        if (error.status === 404) await missingRun();
        else if (state.source === source) showError(error.message, "connection");
      }
    };
  }

  async function missingRun() {
    closeSource();
    state.selected = null;
    state.cursor = 0;
    rememberRun(null);
    clearActivity();
    $("activity-empty").textContent = "Select a run or start a new fixture release.";
    renderCurrent();
    showError("This run is no longer in server memory. The server may have restarted; start a new run or select one from the refreshed list.");
    try {
      await refreshRuns(false);
      setConnection("idle", "Server ready");
    } catch (error) {
      showError(error.message, "connection");
      setConnection("error", "Server unavailable");
    }
  }

  async function selectRun(id) {
    if (state.loadingRun) return;
    state.loadingRun = true;
    closeSource();
    try {
      const snapshot = await api(runPath(id));
      state.selected = snapshot;
      state.cursor = 0;
      state.manualDisconnect = false;
      clearActivity();
      state.runs.set(snapshot.id, snapshot);
      clearError();
      rememberRun(id);
      renderRuns();
      renderCurrent();
      connectStream();
    } catch (error) {
      if (error.status === 404) await missingRun();
      else {
        showError(error.message);
        setConnection("error", "Connection unavailable");
      }
    } finally {
      state.loadingRun = false;
    }
  }

  async function refreshRuns(refreshSelected = true) {
    if (!state.plan) {
      state.plan = await api("/api/plan");
      renderCurrent();
    }
    const runs = await api("/api/runs");
    state.runs = new Map(runs.map((run) => [run.id, run]));
    renderRuns();
    if (refreshSelected && state.selected) {
      const current = state.runs.get(state.selected.id);
      if (current) applySnapshot(current);
      else await missingRun();
    }
  }

  function setSubmitting(value) {
    state.submitting = value;
    $("start-run").disabled = value;
    const mode = new FormData($("launch-form")).get("mode");
    $("start-run").textContent = value ? "Starting…" : mode === "dry-run" ? "Start safe dry-run" : "Start fixture release";
  }

  function updateModeHint() {
    const mode = new FormData($("launch-form")).get("mode");
    const hint = mode === "dry-run"
      ? "Dry-run skips writes and the interactive approval gate, then continues independent reads."
      : "Execute writes to an isolated fixture workspace and waits for your production decision.";
    const target = $("mode-hint");
    while (target.lastChild && target.lastChild.nodeType === Node.TEXT_NODE) target.lastChild.remove();
    target.append(document.createTextNode(hint));
    setSubmitting(state.submitting);
  }

  $("launch-form").addEventListener("submit", async (event) => {
    event.preventDefault();
    if (state.submitting) return;
    clearError();
    setSubmitting(true);
    const values = new FormData(event.currentTarget);
    try {
      const snapshot = await api("/api/runs", {
        method: "POST",
        body: JSON.stringify({ stack: values.get("stack"), mode: values.get("mode"), fail_at: values.get("fail_at") || null }),
      });
      closeSource();
      state.selected = snapshot;
      state.cursor = 0;
      state.manualDisconnect = false;
      state.runs.set(snapshot.id, snapshot);
      clearActivity();
      rememberRun(snapshot.id);
      renderRuns();
      renderCurrent();
      connectStream();
    } catch (error) {
      showError(error.message);
    } finally {
      setSubmitting(false);
    }
  });

  document.querySelectorAll('input[name="mode"]').forEach((input) => input.addEventListener("change", updateModeHint));

  async function decide(decision) {
    const run = state.selected;
    const approval = run?.approval;
    if (!run || !approval || approval.status !== "pending" || state.deciding) return;
    state.deciding = true;
    clearError();
    updateCountdown();
    try {
      const snapshot = await api(`${runPath(run.id)}/approvals/${encodeURIComponent(approval.id)}`, {
        method: "POST",
        body: JSON.stringify({ decision, artifact_digest: approval.subject.artifact_digest, revision: approval.subject.revision }),
      });
      applySnapshot(snapshot);
    } catch (error) {
      showError(error.message);
      try { applySnapshot(await api(runPath(run.id))); } catch (_) { /* Keep the visible request error. */ }
    } finally {
      state.deciding = false;
      updateCountdown();
    }
  }

  $("approve-run").addEventListener("click", () => decide("approve"));
  $("reject-run").addEventListener("click", () => decide("reject"));
  $("cancel-run").addEventListener("click", async () => {
    const run = state.selected;
    if (!run || terminal.has(run.status) || state.cancelling) return;
    state.cancelling = true;
    clearError();
    renderCurrent();
    try {
      applySnapshot(await api(`${runPath(run.id)}/cancel`, { method: "POST", body: "{}" }));
    } catch (error) {
      showError(error.message);
    } finally {
      state.cancelling = false;
      renderCurrent();
    }
  });

  $("toggle-stream").addEventListener("click", async () => {
    if (!state.selected) return;
    state.manualDisconnect = !state.manualDisconnect;
    if (state.manualDisconnect) {
      closeSource();
      setConnection("idle", "Live view disconnected");
      return;
    }
    try {
      applySnapshot(await api(runPath(state.selected.id)));
      clearError("connection");
      connectStream();
    } catch (error) {
      if (error.status === 404) await missingRun();
      else {
        state.manualDisconnect = true;
        setConnection("error", "Connection unavailable");
        showError(error.message, "connection");
      }
    }
  });

  $("refresh-runs").addEventListener("click", async () => {
    $("refresh-runs").disabled = true;
    try {
      await refreshRuns();
      clearError("connection");
      if (!state.selected) setConnection("idle", "Server ready");
    } catch (error) {
      showError(error.message, "connection");
    } finally {
      $("refresh-runs").disabled = false;
    }
  });
  $("dismiss-error").addEventListener("click", () => clearError());
  window.setInterval(updateCountdown, 1000);
  window.addEventListener("beforeunload", closeSource);

  async function initialize() {
    try {
      const [plan, runs] = await Promise.all([api("/api/plan"), api("/api/runs")]);
      state.plan = plan;
      state.runs = new Map(runs.map((run) => [run.id, run]));
      renderRuns();
      renderCurrent();
      setConnection("idle", "Server ready");
      const requested = new URLSearchParams(window.location.search).get("run");
      if (requested) await selectRun(requested);
      else if (runs.length) {
        const latest = [...runs].sort((a, b) => b.created_at_ms - a.created_at_ms)[0];
        await selectRun(latest.id);
      }
    } catch (error) {
      showError(error.message, "connection");
      setConnection("error", "Server unavailable");
      $("node-list").replaceChildren(element("li", "loading-message", "The plan could not be loaded. Refresh this page after starting the local server."));
    }
  }

  initialize();
})();
