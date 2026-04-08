#![allow(clippy::result_large_err)]
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use desktop_core::{
    ActionRequest, CreateSessionRequest, RuntimeCapabilities, SessionRecord, capability_descriptor,
};
use linux_backend::{BackendOptions, LinuxBackend};
use serde_json::{Value, json};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    sessions: Arc<Mutex<HashMap<String, SessionHandle>>>,
    artifacts_root: PathBuf,
    browser_command: String,
    runtime_base_url: String,
}

struct SessionHandle {
    record: SessionRecord,
    backend: LinuxBackend,
    provider_handle: SessionProviderHandle,
}

enum SessionProviderHandle {
    Xvfb { child: Child },
}

#[tokio::main]
async fn main() {
    let port = arg_value("--port")
        .and_then(|value| value.parse().ok())
        .unwrap_or(4001);
    let artifacts_root = PathBuf::from(
        arg_value("--artifacts-dir").unwrap_or_else(|| "artifacts/runtime".to_string()),
    );
    let browser_command = arg_value("--browser-command").unwrap_or_else(|| "firefox".to_string());
    let runtime_base_url = format!("http://127.0.0.1:{port}");

    let state = AppState {
        sessions: Arc::new(Mutex::new(HashMap::new())),
        artifacts_root,
        browser_command,
        runtime_base_url,
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/sessions", post(create_session))
        .route(
            "/api/sessions/{id}",
            get(get_session).delete(delete_session),
        )
        .route("/api/sessions/{id}/observation", get(get_observation))
        .route(
            "/api/sessions/{id}/actions",
            get(get_available_actions).post(perform_action),
        )
        .route("/api/sessions/{id}/screenshot", get(get_screenshot))
        .with_state(state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind guest runtime");
    println!("guest-runtime listening on http://{}", addr);
    axum::serve(listener, app)
        .await
        .expect("serve guest runtime");
}

fn arg_value(flag: &str) -> Option<String> {
    let mut iter = std::env::args().skip(1);
    while let Some(candidate) = iter.next() {
        if candidate == flag {
            return iter.next();
        }
    }
    None
}

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let sessions = state.sessions.lock().await;
    Json(json!({
        "status": "ok",
        "session_count": sessions.len(),
        "runtime_base_url": state.runtime_base_url,
    }))
}

async fn create_session(
    State(state): State<AppState>,
    Json(request): Json<CreateSessionRequest>,
) -> Response {
    match create_session_impl(&state, request).await {
        Ok(session) => (StatusCode::CREATED, Json(json!({ "session": session }))).into_response(),
        Err((status, error)) => (status, Json(json!({ "error": error }))).into_response(),
    }
}

async fn create_session_impl(
    state: &AppState,
    request: CreateSessionRequest,
) -> Result<SessionRecord, (StatusCode, Value)> {
    if request.provider == "qemu" {
        return Err((
            StatusCode::NOT_IMPLEMENTED,
            json!({
                "code": "provider_unavailable",
                "message": "QEMU/KVM is the production target but is not available in this environment. Use the xvfb provider for local verification.",
                "provider": "qemu"
            }),
        ));
    }
    if request.provider != "xvfb" {
        return Err((
            StatusCode::BAD_REQUEST,
            json!({
                "code": "unsupported_provider",
                "message": format!("Unsupported provider `{}`", request.provider),
            }),
        ));
    }
    if !LinuxBackend::tool_exists("Xvfb") {
        return Err((
            StatusCode::FAILED_DEPENDENCY,
            json!({ "code": "missing_tool", "message": "Xvfb is required for the local sandbox provider" }),
        ));
    }

    let session_id = Uuid::new_v4().to_string();
    let display = next_display(state).await;
    let artifacts_dir = state.artifacts_root.join(&session_id);
    tokio::fs::create_dir_all(&artifacts_dir)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({ "code": "artifacts_dir_failed", "message": error.to_string() }),
            )
        })?;

    let mut child = Command::new("Xvfb")
        .arg(&display)
        .args([
            "-screen",
            "0",
            &format!("{}x{}x24", request.width, request.height),
            "-nolisten",
            "tcp",
            "-ac",
        ])
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| {
            (
                StatusCode::FAILED_DEPENDENCY,
                json!({ "code": "xvfb_spawn_failed", "message": error.to_string() }),
            )
        })?;

    tokio::time::sleep(Duration::from_millis(350)).await;
    if let Some(status) = child.try_wait().map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "code": "xvfb_status_failed", "message": error.to_string() }),
        )
    })? {
        return Err((
            StatusCode::FAILED_DEPENDENCY,
            json!({ "code": "xvfb_early_exit", "message": format!("Xvfb exited early: {status}") }),
        ));
    }

    let backend = LinuxBackend::new(BackendOptions {
        display: display.clone(),
        artifacts_dir: artifacts_dir.clone(),
        browser_command: state.browser_command.clone(),
    });
    let record = SessionRecord {
        id: session_id.clone(),
        provider: "xvfb".to_string(),
        display: Some(display),
        width: request.width,
        height: request.height,
        state: "running".to_string(),
        created_at: chrono::Utc::now(),
        artifacts_dir: artifacts_dir.to_string_lossy().to_string(),
        capabilities: backend.capabilities(),
        browser_command: Some(state.browser_command.clone()),
        runtime_base_url: Some(state.runtime_base_url.clone()),
    };

    state.sessions.lock().await.insert(
        session_id,
        SessionHandle {
            record: record.clone(),
            backend,
            provider_handle: SessionProviderHandle::Xvfb { child },
        },
    );

    Ok(record)
}

async fn get_session(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match session_snapshot(&state, &id).await {
        Some(session) => (StatusCode::OK, Json(json!({ "session": session }))).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "session not found" })),
        )
            .into_response(),
    }
}

async fn delete_session(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let handle = state.sessions.lock().await.remove(&id);
    match handle {
        Some(mut handle) => {
            let SessionProviderHandle::Xvfb { child } = &mut handle.provider_handle;
            let _ = child.kill().await;
            (StatusCode::OK, Json(json!({ "ok": true }))).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "session not found" })),
        )
            .into_response(),
    }
}

async fn get_observation(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let backend = match session_backend(&state, &id).await {
        Some(backend) => backend,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "session not found" })),
            )
                .into_response();
        }
    };
    match backend.observation().await {
        Ok(observation) => (StatusCode::OK, Json(json!(observation))).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error })),
        )
            .into_response(),
    }
}

async fn get_screenshot(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let backend = match session_backend(&state, &id).await {
        Some(backend) => backend,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "session not found" })),
            )
                .into_response();
        }
    };
    match backend.screenshot_png().await {
        Ok((bytes, _path)) => {
            let mut response = Response::new(bytes.into());
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                header::HeaderValue::from_static("image/png"),
            );
            response
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error })),
        )
            .into_response(),
    }
}

async fn get_available_actions(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let handle = match session_record_with_capabilities(&state, &id).await {
        Some(handle) => handle,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "session not found" })),
            )
                .into_response();
        }
    };
    let capabilities = runtime_capabilities(&handle.0.provider, handle.1);
    (StatusCode::OK, Json(json!(capabilities))).into_response()
}

async fn perform_action(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(action): Json<ActionRequest>,
) -> Response {
    let backend = match session_backend(&state, &id).await {
        Some(backend) => backend,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "session not found" })),
            )
                .into_response();
        }
    };
    let receipt = backend.perform_action(action).await;
    (StatusCode::OK, Json(json!(receipt))).into_response()
}

async fn session_snapshot(state: &AppState, id: &str) -> Option<SessionRecord> {
    state
        .sessions
        .lock()
        .await
        .get(id)
        .map(|handle| handle.record.clone())
}

async fn session_backend(state: &AppState, id: &str) -> Option<LinuxBackend> {
    state
        .sessions
        .lock()
        .await
        .get(id)
        .map(|handle| handle.backend.clone())
}

async fn session_record_with_capabilities(
    state: &AppState,
    id: &str,
) -> Option<(SessionRecord, Vec<String>)> {
    state
        .sessions
        .lock()
        .await
        .get(id)
        .map(|handle| (handle.record.clone(), handle.backend.capabilities()))
}

fn runtime_capabilities(provider: &str, enrichments: Vec<String>) -> RuntimeCapabilities {
    capability_descriptor(provider, enrichments)
}

async fn next_display(state: &AppState) -> String {
    let sessions = state.sessions.lock().await;
    for candidate in 90..140 {
        let display = format!(":{candidate}");
        let in_use = sessions
            .values()
            .any(|handle| handle.record.display.as_deref() == Some(display.as_str()));
        if !in_use {
            return display;
        }
    }
    format!(":{}", 140 + sessions.len())
}
