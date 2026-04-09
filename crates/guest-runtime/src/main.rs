#![allow(clippy::result_large_err)]
use std::collections::{BTreeSet, HashMap};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json, Router,
    extract::{Path as AxumPath, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use desktop_core::{
    ActionReceipt, ActionRequest, ArtifactRef, CreateSessionRequest, Observation,
    RuntimeCapabilities, SessionRecord, StructuredError, capability_descriptor,
};
use linux_backend::{BackendOptions, LinuxBackend};
use reqwest::Client;
use serde::de::DeserializeOwned;
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
    http_client: Client,
    qemu_viewer_port: u16,
    qemu_guest_runtime_port: u16,
    qemu_guest_display: String,
    qemu_bridge_probe_timeout: Duration,
    qemu_bridge_probe_interval: Duration,
}

struct SessionHandle {
    record: SessionRecord,
    backend: Option<LinuxBackend>,
    provider_handle: SessionProviderHandle,
    remote_bridge: Option<RemoteBridgeHandle>,
}

enum SessionProviderHandle {
    Xvfb { child: Child },
    ExistingDisplay,
    QemuDocker { container_name: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QemuSessionProfile {
    Product,
    Regression,
}

impl QemuSessionProfile {
    fn from_request(request: &CreateSessionRequest) -> Self {
        match request.qemu_profile.as_deref() {
            Some("regression") => Self::Regression,
            _ => Self::Product,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Product => "product",
            Self::Regression => "regression",
        }
    }
}

#[derive(Clone)]
struct RemoteBridgeHandle {
    base_url: String,
    session_id: Option<String>,
}

#[derive(serde::Deserialize)]
struct BridgeSessionResponse {
    session: SessionRecord,
}

struct QemuBridgeMonitor {
    sessions: Arc<Mutex<HashMap<String, SessionHandle>>>,
    http_client: Client,
    host_runtime_base_url: String,
    guest_display: String,
    browser_command: String,
    qemu_profile: QemuSessionProfile,
    session_id: String,
    width: u32,
    height: u32,
    artifacts_dir: PathBuf,
    remote_runtime_url: String,
    viewer_url: String,
    timeout: Duration,
    interval: Duration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QemuLaunchMode {
    PublishedPorts,
    BridgeNetwork,
}

struct QemuContainerSpec<'a> {
    container_name: &'a str,
    image: &'a str,
    boot: &'a str,
    artifacts_dir: &'a Path,
    viewer_port: u16,
    runtime_port: u16,
    disable_kvm: bool,
}

#[tokio::main]
async fn main() {
    let bind_host = arg_value("--host")
        .or_else(|| std::env::var("ACU_BIND_HOST").ok())
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let port = arg_value("--port")
        .and_then(|value| value.parse().ok())
        .unwrap_or(4001);
    let artifacts_root = PathBuf::from(
        arg_value("--artifacts-dir").unwrap_or_else(|| "artifacts/runtime".to_string()),
    );
    let browser_command = arg_value("--browser-command").unwrap_or_else(|| "firefox".to_string());
    let runtime_base_url = format!("http://127.0.0.1:{port}");
    let qemu_viewer_port = std::env::var("ACU_QEMU_VIEWER_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8006);
    let qemu_guest_runtime_port = std::env::var("ACU_QEMU_GUEST_RUNTIME_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(port);
    let qemu_guest_display =
        std::env::var("ACU_QEMU_GUEST_DISPLAY").unwrap_or_else(|_| ":0".to_string());
    let qemu_bridge_probe_timeout = Duration::from_millis(
        std::env::var("ACU_QEMU_BRIDGE_TIMEOUT_MS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(45_000),
    );
    let qemu_bridge_probe_interval = Duration::from_millis(
        std::env::var("ACU_QEMU_BRIDGE_INTERVAL_MS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(1_000),
    );

    let state = AppState {
        sessions: Arc::new(Mutex::new(HashMap::new())),
        artifacts_root,
        browser_command,
        runtime_base_url,
        http_client: Client::new(),
        qemu_viewer_port,
        qemu_guest_runtime_port,
        qemu_guest_display,
        qemu_bridge_probe_timeout,
        qemu_bridge_probe_interval,
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

    let addr: SocketAddr = format!("{bind_host}:{port}")
        .parse()
        .expect("parse guest runtime bind address");
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
    match request.provider.as_str() {
        "xvfb" => create_xvfb_session(state, request).await,
        "display" => create_existing_display_session(state, request).await,
        "qemu" => create_qemu_session(state, request).await,
        other => Err((
            StatusCode::BAD_REQUEST,
            json!({
                "code": "unsupported_provider",
                "message": format!("Unsupported provider `{other}`"),
            }),
        )),
    }
}

async fn create_xvfb_session(
    state: &AppState,
    request: CreateSessionRequest,
) -> Result<SessionRecord, (StatusCode, Value)> {
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
        qemu_profile: None,
        display: Some(display),
        width: request.width,
        height: request.height,
        state: "running".to_string(),
        created_at: chrono::Utc::now(),
        artifacts_dir: artifacts_dir.to_string_lossy().to_string(),
        capabilities: backend.capabilities(),
        browser_command: Some(state.browser_command.clone()),
        runtime_base_url: Some(state.runtime_base_url.clone()),
        viewer_url: None,
        bridge_status: Some("runtime_ready".to_string()),
        readiness_state: Some("runtime_ready".to_string()),
        bridge_error: None,
    };

    state.sessions.lock().await.insert(
        session_id,
        SessionHandle {
            record: record.clone(),
            backend: Some(backend),
            provider_handle: SessionProviderHandle::Xvfb { child },
            remote_bridge: None,
        },
    );

    Ok(record)
}

async fn create_existing_display_session(
    state: &AppState,
    request: CreateSessionRequest,
) -> Result<SessionRecord, (StatusCode, Value)> {
    let session_id = Uuid::new_v4().to_string();
    let display = request
        .display
        .clone()
        .or_else(|| std::env::var("DISPLAY").ok())
        .unwrap_or_else(|| ":0".to_string());
    let browser_command = request
        .browser_command
        .clone()
        .unwrap_or_else(|| state.browser_command.clone());
    let artifacts_dir = state.artifacts_root.join(&session_id);
    tokio::fs::create_dir_all(&artifacts_dir)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({ "code": "artifacts_dir_failed", "message": error.to_string() }),
            )
        })?;

    let backend = LinuxBackend::new(BackendOptions {
        display: display.clone(),
        artifacts_dir: artifacts_dir.clone(),
        browser_command: browser_command.clone(),
    });
    let record = SessionRecord {
        id: session_id.clone(),
        provider: "display".to_string(),
        qemu_profile: None,
        display: Some(display),
        width: request.width,
        height: request.height,
        state: "running".to_string(),
        created_at: chrono::Utc::now(),
        artifacts_dir: artifacts_dir.to_string_lossy().to_string(),
        capabilities: backend.capabilities(),
        browser_command: Some(browser_command),
        runtime_base_url: Some(state.runtime_base_url.clone()),
        viewer_url: None,
        bridge_status: Some("runtime_ready".to_string()),
        readiness_state: Some("runtime_ready".to_string()),
        bridge_error: None,
    };

    state.sessions.lock().await.insert(
        session_id,
        SessionHandle {
            record: record.clone(),
            backend: Some(backend),
            provider_handle: SessionProviderHandle::ExistingDisplay,
            remote_bridge: None,
        },
    );

    Ok(record)
}

async fn create_qemu_session(
    state: &AppState,
    request: CreateSessionRequest,
) -> Result<SessionRecord, (StatusCode, Value)> {
    if !LinuxBackend::tool_exists("docker") {
        return Err((
            StatusCode::FAILED_DEPENDENCY,
            json!({
                "code": "missing_tool",
                "message": "Docker is required for the qemu container provider in this environment",
            }),
        ));
    }

    let qemu_profile = QemuSessionProfile::from_request(&request);
    let session_id = Uuid::new_v4().to_string();
    let artifacts_dir = state.artifacts_root.join(&session_id);
    tokio::fs::create_dir_all(&artifacts_dir)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({ "code": "artifacts_dir_failed", "message": error.to_string() }),
            )
        })?;
    let absolute_artifacts_dir = std::fs::canonicalize(&artifacts_dir).map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "code": "artifacts_dir_canonicalize_failed", "message": error.to_string() }),
        )
    })?;

    let container_name = format!("acu-qemu-{}", &session_id[..12]);
    let image = request
        .container_image
        .clone()
        .or_else(|| std::env::var("ACU_QEMU_CONTAINER_IMAGE").ok())
        .unwrap_or_else(|| "qemux/qemu".to_string());
    let boot = request
        .boot
        .clone()
        .or_else(|| std::env::var("ACU_QEMU_BOOT").ok())
        .unwrap_or_else(|| "alpine".to_string());
    let disable_kvm = request
        .disable_kvm
        .unwrap_or_else(|| !Path::new("/dev/kvm").exists());

    let container_spec = QemuContainerSpec {
        container_name: &container_name,
        image: &image,
        boot: &boot,
        artifacts_dir: &absolute_artifacts_dir,
        viewer_port: state.qemu_viewer_port,
        runtime_port: state.qemu_guest_runtime_port,
        disable_kvm,
    };
    let launch_mode = match launch_qemu_container(&container_spec).await {
        Ok(mode) => mode,
        Err(message) => {
            return Err((
                StatusCode::FAILED_DEPENDENCY,
                json!({
                    "code": "qemu_container_launch_failed",
                    "message": message,
                }),
            ));
        }
    };

    tokio::time::sleep(Duration::from_secs(8)).await;
    let running = docker_output(&["inspect", "-f", "{{.State.Running}}", &container_name]).await?;
    if running.trim() != "true" {
        let logs = docker_output(&["logs", &container_name])
            .await
            .unwrap_or_default();
        let _ = docker_output(&["rm", "-f", &container_name]).await;
        return Err((
            StatusCode::FAILED_DEPENDENCY,
            json!({
                "code": "qemu_container_not_running",
                "message": "qemu container exited before the viewer became available",
                "logs": logs,
            }),
        ));
    }
    let container_ip = docker_container_ip(&container_name).await?;
    let viewer_port = docker_mapped_port(&container_name, state.qemu_viewer_port).await?;
    let runtime_port = docker_mapped_port(&container_name, state.qemu_guest_runtime_port).await?;
    let viewer_url = resolve_qemu_endpoint(viewer_port, &container_ip, state.qemu_viewer_port)
        .ok_or_else(|| {
            (
                StatusCode::FAILED_DEPENDENCY,
                json!({
                    "code": "qemu_container_ip_missing",
                    "message": "qemu container started but did not expose a viewer port or bridge-network IP",
                }),
            )
        })?;
    let remote_runtime_url = match launch_mode {
        QemuLaunchMode::PublishedPorts => {
            runtime_port.map(|port| format!("http://127.0.0.1:{port}"))
        }
        QemuLaunchMode::BridgeNetwork => {
            resolve_qemu_endpoint(runtime_port, &container_ip, state.qemu_guest_runtime_port)
        }
    };
    let mut capabilities = vec![
        "vm".to_string(),
        "viewer".to_string(),
        "qemu_container".to_string(),
        format!("qemu_profile:{}", qemu_profile.as_str()),
    ];
    if launch_mode == QemuLaunchMode::BridgeNetwork {
        capabilities.push("bridge_network_access".to_string());
    }
    if remote_runtime_url.is_some() {
        capabilities.push("guest_runtime_http".to_string());
    }
    let bridge_status = if remote_runtime_url.is_some() {
        "bridge_waiting".to_string()
    } else {
        "viewer_only".to_string()
    };
    let record = SessionRecord {
        id: session_id.clone(),
        provider: "qemu".to_string(),
        qemu_profile: Some(qemu_profile.as_str().to_string()),
        display: None,
        width: request.width,
        height: request.height,
        state: "running".to_string(),
        created_at: chrono::Utc::now(),
        artifacts_dir: artifacts_dir.to_string_lossy().to_string(),
        capabilities,
        browser_command: Some(state.browser_command.clone()),
        runtime_base_url: None,
        viewer_url: Some(viewer_url.clone()),
        bridge_status: Some(bridge_status),
        readiness_state: Some("booting".to_string()),
        bridge_error: None,
    };

    state.sessions.lock().await.insert(
        session_id.clone(),
        SessionHandle {
            record: record.clone(),
            backend: None,
            provider_handle: SessionProviderHandle::QemuDocker { container_name },
            remote_bridge: remote_runtime_url
                .as_ref()
                .map(|base_url| RemoteBridgeHandle {
                    base_url: base_url.clone(),
                    session_id: None,
                }),
        },
    );

    if let Some(remote_runtime_url) = remote_runtime_url {
        tokio::spawn(monitor_qemu_bridge(QemuBridgeMonitor {
            sessions: state.sessions.clone(),
            http_client: state.http_client.clone(),
            host_runtime_base_url: state.runtime_base_url.clone(),
            guest_display: state.qemu_guest_display.clone(),
            browser_command: state.browser_command.clone(),
            qemu_profile,
            session_id,
            width: request.width,
            height: request.height,
            artifacts_dir,
            remote_runtime_url,
            viewer_url,
            timeout: state.qemu_bridge_probe_timeout,
            interval: state.qemu_bridge_probe_interval,
        }));
    }

    Ok(record)
}

async fn monitor_qemu_bridge(monitor: QemuBridgeMonitor) {
    let started_at = chrono::Utc::now();
    let deadline = tokio::time::Instant::now() + monitor.timeout;
    let mut attempts = 0usize;
    let mut last_error = String::new();

    while tokio::time::Instant::now() < deadline {
        attempts += 1;
        if monitor.qemu_profile == QemuSessionProfile::Product
            && monitor
                .http_client
                .get(&monitor.viewer_url)
                .send()
                .await
                .map(|response| response.status().is_success())
                .unwrap_or(false)
        {
            let mut guard = monitor.sessions.lock().await;
            if let Some(handle) = guard.get_mut(&monitor.session_id) {
                promote_readiness(&mut handle.record, "desktop_ready");
            }
        }
        {
            let mut guard = monitor.sessions.lock().await;
            if let Some(handle) = guard.get_mut(&monitor.session_id) {
                promote_readiness(&mut handle.record, "bridge_listening");
            }
        }
        let health = bridge_json::<Value>(
            &monitor.http_client,
            &monitor.remote_runtime_url,
            "/health",
            None,
        )
        .await;
        match health {
            Ok(_) => {
                {
                    let mut guard = monitor.sessions.lock().await;
                    if let Some(handle) = guard.get_mut(&monitor.session_id) {
                        promote_readiness(&mut handle.record, "bridge_attached");
                    }
                }
                let create_request = CreateSessionRequest {
                    provider: match monitor.qemu_profile {
                        QemuSessionProfile::Product => "display".to_string(),
                        QemuSessionProfile::Regression => "xvfb".to_string(),
                    },
                    width: monitor.width,
                    height: monitor.height,
                    display: (monitor.qemu_profile == QemuSessionProfile::Product)
                        .then(|| monitor.guest_display.clone()),
                    browser_command: Some(monitor.browser_command.clone()),
                    boot: None,
                    container_image: None,
                    disable_kvm: None,
                    qemu_profile: None,
                    shared_host_path: None,
                };
                match bridge_json::<BridgeSessionResponse>(
                    &monitor.http_client,
                    &monitor.remote_runtime_url,
                    "/api/sessions",
                    Some(&create_request),
                )
                .await
                {
                    Ok(response) => {
                        let remote_session_id = response.session.id.clone();
                        let remote_capabilities = response.session.capabilities.clone();
                        let mut guard = monitor.sessions.lock().await;
                        if let Some(handle) = guard.get_mut(&monitor.session_id) {
                            if let Some(remote_bridge) = handle.remote_bridge.as_mut() {
                                remote_bridge.session_id = Some(remote_session_id);
                            }
                            handle.record.runtime_base_url =
                                Some(monitor.host_runtime_base_url.clone());
                            handle.record.bridge_status = Some("runtime_ready".to_string());
                            promote_readiness(&mut handle.record, "runtime_ready");
                            handle.record.bridge_error = None;
                            handle.record.capabilities = merge_capabilities(
                                &handle.record.capabilities,
                                &remote_capabilities,
                            );
                        }
                        return;
                    }
                    Err(error) => {
                        last_error = error;
                    }
                }
            }
            Err(error) => {
                last_error = error;
            }
        }
        tokio::time::sleep(monitor.interval).await;
    }

    let diagnostics_path = monitor.artifacts_dir.join("qemu-bridge-diagnostics.json");
    let artifact_path = diagnostics_path.to_string_lossy().to_string();
    let payload = json!({
        "session_id": monitor.session_id,
        "bridge_status": "failed",
        "remote_runtime_url": monitor.remote_runtime_url,
        "attempts": attempts,
        "started_at": started_at,
        "finished_at": chrono::Utc::now(),
        "last_error": last_error,
    });
    let _ = tokio::fs::write(
        &diagnostics_path,
        serde_json::to_vec_pretty(&payload).unwrap_or_default(),
    )
    .await;
    let bridge_error = StructuredError {
        code: "qemu_bridge_attach_failed".to_string(),
        message: "QEMU guest runtime bridge did not become ready in time".to_string(),
        retryable: false,
        category: "provider".to_string(),
        details: json!({
            "remote_runtime_url": monitor.remote_runtime_url,
            "attempts": attempts,
            "last_error": last_error,
        }),
        artifact_refs: vec![ArtifactRef {
            kind: "qemu_bridge_diagnostics".to_string(),
            path: artifact_path,
            mime_type: Some("application/json".to_string()),
        }],
    };

    let mut guard = monitor.sessions.lock().await;
    if let Some(handle) = guard.get_mut(&monitor.session_id) {
        handle.record.bridge_status = Some("failed".to_string());
        promote_readiness(&mut handle.record, "failed");
        handle.record.bridge_error = Some(bridge_error);
    }
}

async fn docker_output(args: &[&str]) -> Result<String, (StatusCode, Value)> {
    let output = Command::new("docker")
        .args(args)
        .output()
        .await
        .map_err(|error| {
            (
                StatusCode::FAILED_DEPENDENCY,
                json!({ "code": "docker_command_failed", "message": error.to_string() }),
            )
        })?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err((
            StatusCode::FAILED_DEPENDENCY,
            json!({
                "code": "docker_command_failed",
                "args": args,
                "stderr": String::from_utf8_lossy(&output.stderr),
            }),
        ))
    }
}

async fn launch_qemu_container(spec: &QemuContainerSpec<'_>) -> Result<QemuLaunchMode, String> {
    let primary = docker_run_qemu_container(spec, QemuLaunchMode::PublishedPorts).await?;
    if primary.status.success() {
        return Ok(QemuLaunchMode::PublishedPorts);
    }

    let primary_stderr = String::from_utf8_lossy(&primary.stderr).into_owned();
    if !should_retry_qemu_without_published_ports(&primary_stderr) {
        return Err(primary_stderr);
    }

    let fallback = docker_run_qemu_container(spec, QemuLaunchMode::BridgeNetwork).await?;
    if fallback.status.success() {
        Ok(QemuLaunchMode::BridgeNetwork)
    } else {
        let fallback_stderr = String::from_utf8_lossy(&fallback.stderr);
        Err(format!(
            "{}\nbridge-network retry failed:\n{}",
            primary_stderr, fallback_stderr
        ))
    }
}

async fn docker_run_qemu_container(
    spec: &QemuContainerSpec<'_>,
    launch_mode: QemuLaunchMode,
) -> Result<std::process::Output, String> {
    let mut args = vec![
        "run".to_string(),
        "-d".to_string(),
        "--rm".to_string(),
        "--name".to_string(),
        spec.container_name.to_string(),
    ];
    if launch_mode == QemuLaunchMode::PublishedPorts {
        args.push("-p".to_string());
        args.push(format!("127.0.0.1::{}", spec.viewer_port));
        args.push("-p".to_string());
        args.push(format!("127.0.0.1::{}", spec.runtime_port));
    }
    args.push("-e".to_string());
    args.push(format!("BOOT={}", spec.boot));
    args.push("-e".to_string());
    args.push(format!("USER_PORTS=22,{}", spec.runtime_port));
    args.push("-v".to_string());
    args.push(format!("{}:/storage", spec.artifacts_dir.to_string_lossy()));
    if spec.disable_kvm {
        args.push("-e".to_string());
        args.push("KVM=N".to_string());
    } else {
        args.push("--device".to_string());
        args.push("/dev/kvm".to_string());
    }
    if Path::new("/dev/net/tun").exists() {
        args.push("--device".to_string());
        args.push("/dev/net/tun".to_string());
    }
    args.push("--cap-add".to_string());
    args.push("NET_ADMIN".to_string());
    args.push(spec.image.to_string());

    Command::new("docker")
        .args(&args)
        .output()
        .await
        .map_err(|error| error.to_string())
}

async fn docker_mapped_port(
    container_name: &str,
    container_port: u16,
) -> Result<Option<u16>, (StatusCode, Value)> {
    let output = Command::new("docker")
        .args(["port", container_name, &format!("{container_port}/tcp")])
        .output()
        .await
        .map_err(|error| {
            (
                StatusCode::FAILED_DEPENDENCY,
                json!({ "code": "docker_command_failed", "message": error.to_string() }),
            )
        })?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(parse_published_port(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

async fn docker_container_ip(container_name: &str) -> Result<String, (StatusCode, Value)> {
    Ok(docker_output(&[
        "inspect",
        "-f",
        "{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}",
        container_name,
    ])
    .await?
    .trim()
    .to_string())
}

fn parse_published_port(output: &str) -> Option<u16> {
    output
        .lines()
        .filter_map(|line| line.trim().rsplit(':').next())
        .find_map(|port| port.parse::<u16>().ok())
}

fn resolve_qemu_endpoint(
    published_port: Option<u16>,
    container_ip: &str,
    container_port: u16,
) -> Option<String> {
    if let Some(port) = published_port {
        return Some(format!("http://127.0.0.1:{port}"));
    }
    let trimmed_ip = container_ip.trim();
    if trimmed_ip.is_empty() {
        None
    } else {
        Some(format!("http://{trimmed_ip}:{container_port}"))
    }
}

fn should_retry_qemu_without_published_ports(stderr: &str) -> bool {
    let stderr = stderr.to_ascii_lowercase();
    stderr.contains("unable to enable dnat rule")
        || stderr.contains("no chain/target/match by that name")
        || stderr.contains("driver failed programming external connectivity")
}

async fn bridge_json<T: DeserializeOwned>(
    client: &Client,
    base_url: &str,
    path: &str,
    body: Option<&CreateSessionRequest>,
) -> Result<T, String> {
    let request = if let Some(body) = body {
        client.post(format!("{base_url}{path}")).json(body)
    } else {
        client.get(format!("{base_url}{path}"))
    };
    let response = request.send().await.map_err(|error| error.to_string())?;
    let status = response.status();
    let text = response.text().await.map_err(|error| error.to_string())?;
    if !status.is_success() {
        return Err(format!(
            "bridge request to {path} failed with {status}: {text}"
        ));
    }
    serde_json::from_str(&text).map_err(|error| error.to_string())
}

fn ready_remote_bridge(remote_bridge: &RemoteBridgeHandle) -> Option<&RemoteBridgeHandle> {
    remote_bridge.session_id.as_ref()?;
    Some(remote_bridge)
}

fn merge_capabilities(left: &[String], right: &[String]) -> Vec<String> {
    let mut merged = BTreeSet::new();
    merged.extend(left.iter().cloned());
    merged.extend(right.iter().cloned());
    merged.into_iter().collect()
}

fn readiness_rank(stage: &str) -> usize {
    match stage {
        "booting" => 0,
        "desktop_ready" => 1,
        "bridge_listening" => 2,
        "bridge_attached" => 3,
        "runtime_ready" => 4,
        "failed" => 5,
        _ => 0,
    }
}

fn promote_readiness(record: &mut SessionRecord, next_stage: &str) {
    let current_rank = record
        .readiness_state
        .as_deref()
        .map(readiness_rank)
        .unwrap_or(0);
    if readiness_rank(next_stage) >= current_rank {
        record.readiness_state = Some(next_stage.to_string());
    }
}

async fn proxy_bridge_json<T: DeserializeOwned>(
    state: &AppState,
    remote_bridge: &RemoteBridgeHandle,
    endpoint: &str,
    action: Option<&ActionRequest>,
) -> Result<T, StructuredError> {
    let remote_session_id = remote_bridge
        .session_id
        .as_ref()
        .ok_or_else(|| StructuredError {
            code: "provider_bridge_unavailable".to_string(),
            message: "remote bridge session is not ready".to_string(),
            retryable: true,
            category: "provider".to_string(),
            details: json!({ "base_url": remote_bridge.base_url }),
            artifact_refs: vec![],
        })?;
    let url = format!(
        "{}/api/sessions/{}/{}",
        remote_bridge.base_url, remote_session_id, endpoint
    );
    let request = if let Some(action) = action {
        state.http_client.post(url).json(action)
    } else {
        state.http_client.get(url)
    };
    let response = request.send().await.map_err(|error| StructuredError {
        code: "remote_bridge_request_failed".to_string(),
        message: error.to_string(),
        retryable: true,
        category: "provider".to_string(),
        details: json!({ "base_url": remote_bridge.base_url, "endpoint": endpoint }),
        artifact_refs: vec![],
    })?;
    let status = response.status();
    let text = response.text().await.map_err(|error| StructuredError {
        code: "remote_bridge_response_failed".to_string(),
        message: error.to_string(),
        retryable: true,
        category: "provider".to_string(),
        details: json!({ "base_url": remote_bridge.base_url, "endpoint": endpoint }),
        artifact_refs: vec![],
    })?;
    if !status.is_success() {
        return Err(StructuredError {
            code: "remote_bridge_status_failed".to_string(),
            message: format!("remote bridge returned {status}"),
            retryable: true,
            category: "provider".to_string(),
            details: json!({
                "base_url": remote_bridge.base_url,
                "endpoint": endpoint,
                "status": status.as_u16(),
                "body": text,
            }),
            artifact_refs: vec![],
        });
    }
    serde_json::from_str(&text).map_err(|error| StructuredError {
        code: "remote_bridge_decode_failed".to_string(),
        message: error.to_string(),
        retryable: true,
        category: "provider".to_string(),
        details: json!({ "base_url": remote_bridge.base_url, "endpoint": endpoint, "body": text }),
        artifact_refs: vec![],
    })
}

async fn proxy_bridge_bytes(
    state: &AppState,
    remote_bridge: &RemoteBridgeHandle,
    endpoint: &str,
) -> Result<Vec<u8>, StructuredError> {
    let remote_session_id = remote_bridge
        .session_id
        .as_ref()
        .ok_or_else(|| StructuredError {
            code: "provider_bridge_unavailable".to_string(),
            message: "remote bridge session is not ready".to_string(),
            retryable: true,
            category: "provider".to_string(),
            details: json!({ "base_url": remote_bridge.base_url }),
            artifact_refs: vec![],
        })?;
    let url = format!(
        "{}/api/sessions/{}/{}",
        remote_bridge.base_url, remote_session_id, endpoint
    );
    let response = state
        .http_client
        .get(url)
        .send()
        .await
        .map_err(|error| StructuredError {
            code: "remote_bridge_request_failed".to_string(),
            message: error.to_string(),
            retryable: true,
            category: "provider".to_string(),
            details: json!({ "base_url": remote_bridge.base_url, "endpoint": endpoint }),
            artifact_refs: vec![],
        })?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(StructuredError {
            code: "remote_bridge_status_failed".to_string(),
            message: format!("remote bridge returned {status}"),
            retryable: true,
            category: "provider".to_string(),
            details: json!({
                "base_url": remote_bridge.base_url,
                "endpoint": endpoint,
                "status": status.as_u16(),
                "body": body,
            }),
            artifact_refs: vec![],
        });
    }
    response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|error| StructuredError {
            code: "remote_bridge_bytes_failed".to_string(),
            message: error.to_string(),
            retryable: true,
            category: "provider".to_string(),
            details: json!({ "base_url": remote_bridge.base_url, "endpoint": endpoint }),
            artifact_refs: vec![],
        })
}

async fn get_session(State(state): State<AppState>, AxumPath(id): AxumPath<String>) -> Response {
    match session_snapshot(&state, &id).await {
        Some(session) => (StatusCode::OK, Json(json!({ "session": session }))).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "session not found" })),
        )
            .into_response(),
    }
}

async fn delete_session(State(state): State<AppState>, AxumPath(id): AxumPath<String>) -> Response {
    let handle = state.sessions.lock().await.remove(&id);
    match handle {
        Some(mut handle) => {
            if let Some(remote_bridge) = handle.remote_bridge.as_ref()
                && let Some(remote_session_id) = remote_bridge.session_id.as_ref()
            {
                let _ = state
                    .http_client
                    .delete(format!(
                        "{}/api/sessions/{}",
                        remote_bridge.base_url, remote_session_id
                    ))
                    .send()
                    .await;
            }
            match &mut handle.provider_handle {
                SessionProviderHandle::Xvfb { child } => {
                    let _ = child.kill().await;
                }
                SessionProviderHandle::ExistingDisplay => {}
                SessionProviderHandle::QemuDocker { container_name } => {
                    let _ = docker_output(&["rm", "-f", container_name]).await;
                }
            }
            (StatusCode::OK, Json(json!({ "ok": true }))).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "session not found" })),
        )
            .into_response(),
    }
}

async fn get_observation(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let session = match session_clone(&state, &id).await {
        Some(session) => session,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "session not found" })),
            )
                .into_response();
        }
    };
    if let Some(backend) = session.backend {
        match backend.observation().await {
            Ok(observation) => (StatusCode::OK, Json(json!(observation))).into_response(),
            Err(error) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error })),
            )
                .into_response(),
        }
    } else if let Some(remote_bridge) = session.remote_bridge.as_ref().and_then(ready_remote_bridge)
    {
        match proxy_bridge_json::<Observation>(&state, remote_bridge, "observation", None).await {
            Ok(observation) => (StatusCode::OK, Json(json!(observation))).into_response(),
            Err(error) => {
                (StatusCode::BAD_GATEWAY, Json(json!({ "error": error }))).into_response()
            }
        }
    } else {
        provider_bridge_unavailable_response(
            &session.record,
            "observation requires a guest runtime bridge inside the VM",
        )
    }
}

async fn get_screenshot(State(state): State<AppState>, AxumPath(id): AxumPath<String>) -> Response {
    let session = match session_clone(&state, &id).await {
        Some(session) => session,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "session not found" })),
            )
                .into_response();
        }
    };
    if let Some(backend) = session.backend {
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
    } else if let Some(remote_bridge) = session.remote_bridge.as_ref().and_then(ready_remote_bridge)
    {
        match proxy_bridge_bytes(&state, remote_bridge, "screenshot").await {
            Ok(bytes) => {
                let mut response = Response::new(bytes.into());
                response.headers_mut().insert(
                    header::CONTENT_TYPE,
                    header::HeaderValue::from_static("image/png"),
                );
                response
            }
            Err(error) => {
                (StatusCode::BAD_GATEWAY, Json(json!({ "error": error }))).into_response()
            }
        }
    } else {
        provider_bridge_unavailable_response(
            &session.record,
            "screenshot capture requires a guest runtime bridge inside the VM",
        )
    }
}

async fn get_available_actions(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let session = match session_clone(&state, &id).await {
        Some(session) => session,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "session not found" })),
            )
                .into_response();
        }
    };
    if let Some(remote_bridge) = session.remote_bridge.as_ref().and_then(ready_remote_bridge) {
        match proxy_bridge_json::<RuntimeCapabilities>(&state, remote_bridge, "actions", None).await
        {
            Ok(mut capabilities) => {
                capabilities.provider = session.record.provider.clone();
                capabilities.vm_mode = if session.record.provider == "qemu" {
                    "qemu".to_string()
                } else {
                    capabilities.vm_mode
                };
                capabilities.enrichments =
                    merge_capabilities(&capabilities.enrichments, &session.record.capabilities);
                return (StatusCode::OK, Json(json!(capabilities))).into_response();
            }
            Err(error) => {
                return (StatusCode::BAD_GATEWAY, Json(json!({ "error": error }))).into_response();
            }
        }
    }
    let mut capabilities = runtime_capabilities(
        &session.record.provider,
        session.record.capabilities.clone(),
    );
    if session.backend.is_none() {
        capabilities.actions.clear();
        capabilities.browser_mode = "viewer_only".to_string();
    }
    (StatusCode::OK, Json(json!(capabilities))).into_response()
}

async fn perform_action(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(action): Json<ActionRequest>,
) -> Response {
    let session = match session_clone(&state, &id).await {
        Some(session) => session,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "session not found" })),
            )
                .into_response();
        }
    };
    if let Some(backend) = session.backend {
        let receipt = backend.perform_action(action).await;
        (StatusCode::OK, Json(json!(receipt))).into_response()
    } else if let Some(remote_bridge) = session.remote_bridge.as_ref().and_then(ready_remote_bridge)
    {
        match proxy_bridge_json::<ActionReceipt>(&state, remote_bridge, "actions", Some(&action))
            .await
        {
            Ok(receipt) => (StatusCode::OK, Json(json!(receipt))).into_response(),
            Err(error) => {
                (StatusCode::BAD_GATEWAY, Json(json!({ "error": error }))).into_response()
            }
        }
    } else {
        provider_bridge_unavailable_response(
            &session.record,
            "actions require a guest runtime bridge inside the VM",
        )
    }
}

fn provider_bridge_unavailable_response(record: &SessionRecord, reason: &str) -> Response {
    let error = StructuredError {
        code: "provider_bridge_unavailable".to_string(),
        message: reason.to_string(),
        retryable: false,
        category: "provider".to_string(),
        details: json!({
            "provider": record.provider,
            "qemu_profile": record.qemu_profile,
            "viewer_url": record.viewer_url,
            "bridge_status": record.bridge_status,
            "readiness_state": record.readiness_state,
            "bridge_error": record.bridge_error,
        }),
        artifact_refs: record
            .bridge_error
            .as_ref()
            .map(|error| error.artifact_refs.clone())
            .unwrap_or_default(),
    };
    (StatusCode::CONFLICT, Json(json!({ "error": error }))).into_response()
}

async fn session_snapshot(state: &AppState, id: &str) -> Option<SessionRecord> {
    state
        .sessions
        .lock()
        .await
        .get(id)
        .map(|handle| handle.record.clone())
}

async fn session_clone(state: &AppState, id: &str) -> Option<SessionHandleClone> {
    state
        .sessions
        .lock()
        .await
        .get(id)
        .map(|handle| SessionHandleClone {
            record: handle.record.clone(),
            backend: handle.backend.clone(),
            remote_bridge: handle.remote_bridge.clone(),
        })
}

struct SessionHandleClone {
    record: SessionRecord,
    backend: Option<LinuxBackend>,
    remote_bridge: Option<RemoteBridgeHandle>,
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

#[cfg(test)]
mod tests {
    use super::{
        merge_capabilities, parse_published_port, resolve_qemu_endpoint,
        should_retry_qemu_without_published_ports,
    };

    #[test]
    fn parses_published_port_from_docker_output() {
        assert_eq!(parse_published_port("127.0.0.1:49153\n"), Some(49153));
        assert_eq!(
            parse_published_port("0.0.0.0:49153\n:::49153\n"),
            Some(49153)
        );
    }

    #[test]
    fn merges_capabilities_without_duplicates() {
        let merged = merge_capabilities(
            &["vm".to_string(), "viewer".to_string()],
            &["viewer".to_string(), "shell".to_string()],
        );
        assert_eq!(
            merged,
            vec!["shell".to_string(), "viewer".to_string(), "vm".to_string()]
        );
    }

    #[test]
    fn resolves_qemu_endpoint_from_published_port_or_bridge_ip() {
        assert_eq!(
            resolve_qemu_endpoint(Some(49153), "172.17.0.2", 4001),
            Some("http://127.0.0.1:49153".to_string())
        );
        assert_eq!(
            resolve_qemu_endpoint(None, "172.17.0.2", 4001),
            Some("http://172.17.0.2:4001".to_string())
        );
        assert_eq!(resolve_qemu_endpoint(None, "", 4001), None);
    }

    #[test]
    fn detects_nat_failures_that_need_bridge_retry() {
        assert!(should_retry_qemu_without_published_ports(
            "Unable to enable DNAT rule: iptables: No chain/target/match by that name"
        ));
        assert!(should_retry_qemu_without_published_ports(
            "driver failed programming external connectivity on endpoint"
        ));
        assert!(!should_retry_qemu_without_published_ports(
            "manifest for qemux/qemu:missing not found"
        ));
    }
}
