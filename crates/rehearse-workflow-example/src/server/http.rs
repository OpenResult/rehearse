use super::model::{ControlError, CreateRun, DecisionRequest, OwnedEvent, PublicError};
use super::state::{Run, ServerState};
use super::workflow;
use axum::body::Body;
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::stream::{self, Stream};
use serde::Deserialize;
use std::collections::VecDeque;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

pub fn router(state: Arc<ServerState>, origin: String) -> Router {
    Router::new()
        .route(
            "/",
            get(|| async { Html(include_str!("../../web/index.html")) }),
        )
        .route(
            "/app.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../../web/app.js"),
                )
            }),
        )
        .route(
            "/style.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    include_str!("../../web/style.css"),
                )
            }),
        )
        .route(
            "/api/plan",
            get(|| async { Json(workflow::plan_snapshot()) }),
        )
        .route("/api/runs", get(list_runs).post(create_run))
        .route("/api/runs/{id}", get(get_run))
        .route("/api/runs/{id}/events", get(events))
        .route("/api/runs/{id}/approvals/{gate_id}", post(decide))
        .route("/api/runs/{id}/cancel", post(cancel))
        .fallback(|| async { api_error(StatusCode::NOT_FOUND, "not_found", "Route not found") })
        .layer(DefaultBodyLimit::max(8192))
        .layer(middleware::from_fn_with_state(origin, local_requests))
        .with_state(state)
}

async fn local_requests(
    State(origin): State<String>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let port = origin.rsplit(':').next().unwrap_or("");
    let allowed = [
        origin.trim_start_matches("http://").to_owned(),
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        format!("[::1]:{port}"),
    ];
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok());
    if !host.is_some_and(|host| allowed.iter().any(|candidate| candidate == host)) {
        return api_error(
            StatusCode::FORBIDDEN,
            "local_only",
            "Use the server's printed loopback URL",
        );
    }
    if request.method() != axum::http::Method::GET && request.method() != axum::http::Method::HEAD {
        let foreign_origin = request.headers().get(header::ORIGIN).is_some_and(|value| {
            value.to_str().map_or(true, |value| {
                !allowed.iter().any(|host| value == format!("http://{host}"))
            })
        });
        let cross_site = request
            .headers()
            .get("sec-fetch-site")
            .is_some_and(|value| value == "cross-site");
        if foreign_origin || cross_site {
            return api_error(
                StatusCode::FORBIDDEN,
                "origin_rejected",
                "Mutating requests must come from this local UI",
            );
        }
    }
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; base-uri 'none'; frame-ancestors 'none'; form-action 'self'"));
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

async fn list_runs(State(state): State<Arc<ServerState>>) -> Json<Vec<super::model::RunSnapshot>> {
    Json(state.list_runs())
}

async fn create_run(
    State(state): State<Arc<ServerState>>,
    input: Result<Json<CreateRun>, JsonRejection>,
) -> Result<impl IntoResponse, ApiError> {
    let Json(input) = input.map_err(bad_json)?;
    let snapshot = state.create_run(input)?;
    Ok((
        StatusCode::ACCEPTED,
        [(header::LOCATION, format!("/api/runs/{}", snapshot.id))],
        Json(snapshot),
    ))
}

async fn get_run(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(state.get_run(&id)?.snapshot()))
}

async fn decide(
    State(state): State<Arc<ServerState>>,
    Path((id, gate_id)): Path<(String, String)>,
    input: Result<Json<DecisionRequest>, JsonRejection>,
) -> Result<impl IntoResponse, ApiError> {
    let Json(input) = input.map_err(bad_json)?;
    Ok(Json(state.get_run(&id)?.decide(&gate_id, &input)?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

async fn cancel(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    input: Result<Json<Empty>, JsonRejection>,
) -> Result<impl IntoResponse, ApiError> {
    let Json(_) = input.map_err(bad_json)?;
    Ok(Json(state.get_run(&id)?.cancel()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct After {
    after: Option<u64>,
}

struct StreamState {
    run: Arc<Run>,
    cursor: u64,
    changes: watch::Receiver<u64>,
    pending: VecDeque<(bool, OwnedEvent)>,
}

async fn events(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    query: Result<Query<After>, QueryRejection>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let Query(query) = query.map_err(|error| ApiError::bad_request(error.body_text()))?;
    let cursor = match headers.get("last-event-id") {
        Some(value) => value
            .to_str()
            .ok()
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| ApiError::bad_request("Last-Event-ID must be a nonnegative integer"))?,
        None => query.after.unwrap_or(0),
    };
    let run = state.get_run(&id)?;
    if cursor > run.snapshot().last_event_id {
        return Err(ApiError::bad_request("Event cursor is ahead of this run"));
    }
    let changes = run.subscribe();
    let stream_state = StreamState {
        run,
        cursor,
        changes,
        pending: VecDeque::new(),
    };
    let stream = stream::unfold(stream_state, |mut state| async move {
        loop {
            if let Some((reset, event)) = state.pending.pop_front() {
                state.cursor = event.seq;
                let item = Event::default()
                    .event(if reset { "reset" } else { "progress" })
                    .id(event.seq.to_string())
                    .json_data(event)
                    .expect("owned events contain JSON-serializable fields");
                return Some((Ok(item), state));
            }
            let replay = state.run.events_since(state.cursor);
            if let Some(reset) = replay.reset {
                state.pending.push_back((true, reset));
            } else {
                state
                    .pending
                    .extend(replay.events.into_iter().map(|event| (false, event)));
            }
            if !state.pending.is_empty() {
                continue;
            }
            let snapshot = state.run.snapshot();
            if snapshot.status.is_terminal() {
                if snapshot.last_event_id > state.cursor {
                    continue;
                }
                return None;
            }
            if state.changes.changed().await.is_err() {
                return None;
            }
        }
    });
    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(10))
            .text("keep-alive"),
    ))
}

fn bad_json(error: JsonRejection) -> ApiError {
    ApiError {
        status: error.status(),
        code: "invalid_request",
        message: error.body_text(),
    }
}

pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_request",
            message: message.into(),
        }
    }
}
impl From<ControlError> for ApiError {
    fn from(error: ControlError) -> Self {
        let (status, code) = match &error {
            ControlError::Invalid(_) => (StatusCode::BAD_REQUEST, "invalid_request"),
            ControlError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            ControlError::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            ControlError::Capacity => (StatusCode::TOO_MANY_REQUESTS, "capacity"),
            ControlError::Lab(_) => (StatusCode::INTERNAL_SERVER_ERROR, "fixture_error"),
        };
        Self {
            status,
            code,
            message: error.to_string(),
        }
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        api_error(self.status, self.code, &self.message)
    }
}
fn api_error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(serde_json::json!({"error": PublicError { code: code.to_owned(), message: message.to_owned() }}))).into_response()
}
