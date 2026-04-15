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
    routing::get,
};
use desktop_core::{
    ActionReceipt, ActionRequest, ArtifactRef, CreateSessionRequest, LiveDesktopView, Observation,
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

const SCREENSHOT_POLL_INTERVAL_MS: u64 = 3_000;
const QEMU_PRODUCT_DESKTOP_USER: &str = "ubuntu";
const QEMU_PRODUCT_DESKTOP_HOME: &str = "/home/ubuntu";
const QEMU_PRODUCT_RUNTIME_DIR: &str = "/run/user/1000";
const STORAGE_MARKER_FILE: &str = ".inspectors-storage.json";
const STORAGE_OWNER: &str = "inspectors";
const STORAGE_LAYOUT_VERSION: u8 = 1;

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

fn derive_live_desktop_view(record: &SessionRecord) -> LiveDesktopView {
    match record.provider.as_str() {
        "qemu" => {
            let debug_url = record.viewer_url.clone();
            match record.qemu_profile.as_deref() {
                Some("regression") => LiveDesktopView {
                    mode: "screenshot_poll".to_string(),
                    status: if record.bridge_status.as_deref() == Some("runtime_ready") {
                        "ready".to_string()
                    } else {
                        "unavailable".to_string()
                    },
                    provider_surface: "guest_xvfb_screenshot".to_string(),
                    matches_action_plane: true,
                    canonical_url: None,
                    debug_url,
                    reason: Some(
                        "qemu regression keeps the VM viewer as debug-only because the action plane runs inside guest xvfb"
                            .to_string(),
                    ),
                    refresh_interval_ms: Some(SCREENSHOT_POLL_INTERVAL_MS),
                },
                _ => LiveDesktopView {
                    mode: "stream".to_string(),
                    status: if record.viewer_url.is_some() {
                        "ready".to_string()
                    } else {
                        "unavailable".to_string()
                    },
                    provider_surface: "qemu_novnc".to_string(),
                    matches_action_plane: true,
                    canonical_url: None,
                    debug_url,
                    reason: None,
                    refresh_interval_ms: None,
                },
            }
        }
        "xvfb" => LiveDesktopView {
            mode: "screenshot_poll".to_string(),
            status: "ready".to_string(),
            provider_surface: "guest_xvfb_screenshot".to_string(),
            matches_action_plane: true,
            canonical_url: None,
            debug_url: None,
            reason: Some(
                "xvfb is an honest local/dev screenshot fallback without a live desktop stream"
                    .to_string(),
            ),
            refresh_interval_ms: Some(SCREENSHOT_POLL_INTERVAL_MS),
        },
        "display" => LiveDesktopView {
            mode: "screenshot_poll".to_string(),
            status: "ready".to_string(),
            provider_surface: "display_screenshot".to_string(),
            matches_action_plane: true,
            canonical_url: None,
            debug_url: None,
            reason: Some("display sessions expose screenshot polling only".to_string()),
            refresh_interval_ms: Some(SCREENSHOT_POLL_INTERVAL_MS),
        },
        _ => LiveDesktopView {
            mode: "unavailable".to_string(),
            status: "unavailable".to_string(),
            provider_surface: "none".to_string(),
            matches_action_plane: false,
            canonical_url: None,
            debug_url: record.viewer_url.clone(),
            reason: Some("live desktop view is unavailable for this provider".to_string()),
            refresh_interval_ms: None,
        },
    }
}

fn enrich_session_record(record: &SessionRecord) -> SessionRecord {
    let mut enriched = record.clone();
    enriched.live_desktop_view = Some(derive_live_desktop_view(record));
    enriched
}

#[derive(serde::Deserialize)]
struct BridgeSessionResponse {
    session: SessionRecord,
}

#[derive(serde::Deserialize)]
struct EnsuredQemuImage {
    image_path: String,
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
    guest_runtime_binary_path: &'a Path,
    boot_image_path: Option<&'a Path>,
    seed_iso_path: Option<&'a Path>,
    shared_host_path: Option<&'a Path>,
    viewer_port: u16,
    runtime_port: u16,
    disable_kvm: bool,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct StorageOwnershipMarker {
    version: u8,
    owner: String,
    tier: String,
    kind: String,
    created_at: chrono::DateTime<chrono::Utc>,
    session_id: Option<String>,
    provider: Option<String>,
    qemu_profile: Option<String>,
    container_name: Option<String>,
    process_id: Option<u32>,
}

impl StorageOwnershipMarker {
    fn runtime_session(
        session_id: &str,
        provider: &str,
        qemu_profile: Option<&str>,
        container_name: Option<&str>,
        process_id: Option<u32>,
    ) -> Self {
        Self {
            version: STORAGE_LAYOUT_VERSION,
            owner: STORAGE_OWNER.to_string(),
            tier: "runtime".to_string(),
            kind: "session".to_string(),
            created_at: chrono::Utc::now(),
            session_id: Some(session_id.to_string()),
            provider: Some(provider.to_string()),
            qemu_profile: qemu_profile.map(str::to_string),
            container_name: container_name.map(str::to_string),
            process_id,
        }
    }

}

#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    pub bind_host: String,
    pub port: u16,
    pub artifacts_root: PathBuf,
    pub browser_command: String,
    pub qemu_viewer_port: u16,
    pub qemu_guest_runtime_port: u16,
    pub qemu_guest_display: String,
    pub qemu_bridge_probe_timeout: Duration,
    pub qemu_bridge_probe_interval: Duration,
}

impl RuntimeConfig {
    pub fn from_env_and_args() -> Self {
        let bind_host = arg_value("--host")
            .or_else(|| std::env::var("ACU_BIND_HOST").ok())
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let port = arg_value("--port")
            .and_then(|value| value.parse().ok())
            .unwrap_or(4001);
        let artifacts_root = PathBuf::from(
            arg_value("--artifacts-dir").unwrap_or_else(|| "artifacts/runtime".to_string()),
        );
        let browser_command =
            arg_value("--browser-command").unwrap_or_else(|| "firefox".to_string());
        let qemu_viewer_port = std::env::var("ACU_QEMU_VIEWER_PORT")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(8006);
        let qemu_guest_runtime_port = std::env::var("ACU_QEMU_GUEST_RUNTIME_PORT")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(4001);
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

        Self {
            bind_host,
            port,
            artifacts_root,
            browser_command,
            qemu_viewer_port,
            qemu_guest_runtime_port,
            qemu_guest_display,
            qemu_bridge_probe_timeout,
            qemu_bridge_probe_interval,
        }
    }

    pub fn runtime_base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

pub async fn run(config: RuntimeConfig) {
    let RuntimeConfig {
        bind_host,
        port,
        artifacts_root,
        browser_command,
        qemu_viewer_port,
        qemu_guest_runtime_port,
        qemu_guest_display,
        qemu_bridge_probe_timeout,
        qemu_bridge_probe_interval,
    } = config;
    let runtime_base_url = format!("http://127.0.0.1:{port}");

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

    if let Err(error) = ensure_storage_roots(&state.artifacts_root).await {
        panic!("failed to prepare storage roots: {error}");
    }
    cleanup_orphaned_qemu_containers().await;
    janitor_managed_storage(&state.artifacts_root).await;

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/storage/reclaim", axum::routing::post(reclaim_storage))
        .route("/api/sessions", get(list_sessions).post(create_session))
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

fn artifacts_base_root(runtime_root: &Path) -> PathBuf {
    runtime_root
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn qemu_cache_root(runtime_root: &Path) -> PathBuf {
    artifacts_base_root(runtime_root).join("cache").join("qemu")
}

fn qemu_build_root(runtime_root: &Path) -> PathBuf {
    qemu_cache_root(runtime_root).join("_build")
}

fn exports_root(runtime_root: &Path) -> PathBuf {
    artifacts_base_root(runtime_root).join("exports")
}

fn storage_marker_path(path: &Path) -> PathBuf {
    path.join(STORAGE_MARKER_FILE)
}

async fn ensure_storage_roots(runtime_root: &Path) -> std::io::Result<()> {
    tokio::fs::create_dir_all(runtime_root).await?;
    tokio::fs::create_dir_all(qemu_cache_root(runtime_root)).await?;
    tokio::fs::create_dir_all(qemu_build_root(runtime_root)).await?;
    tokio::fs::create_dir_all(exports_root(runtime_root)).await?;
    Ok(())
}

async fn write_storage_marker(
    directory: &Path,
    marker: &StorageOwnershipMarker,
) -> std::io::Result<()> {
    let payload = serde_json::to_vec_pretty(marker)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    tokio::fs::write(
        storage_marker_path(directory),
        payload,
    )
    .await
}

fn read_storage_marker(directory: &Path) -> Option<StorageOwnershipMarker> {
    let marker_path = storage_marker_path(directory);
    let bytes = std::fs::read(marker_path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn marker_matches_inspectors_layout(marker: &StorageOwnershipMarker) -> bool {
    marker.owner == STORAGE_OWNER && marker.version == STORAGE_LAYOUT_VERSION
}

fn process_is_live(process_id: u32) -> bool {
    Path::new("/proc").join(process_id.to_string()).exists()
}

async fn docker_container_exists(container_name: &str) -> bool {
    if !LinuxBackend::tool_exists("docker") {
        return false;
    }
    Command::new("docker")
        .args(["inspect", container_name])
        .output()
        .await
        .map(|output| output.status.success())
        .unwrap_or(false)
}

async fn can_remove_marker_owned_directory(directory: &Path, marker: &StorageOwnershipMarker) -> bool {
    if !marker_matches_inspectors_layout(marker) {
        return false;
    }
    if marker.tier == "cache" || marker.tier == "exports" {
        return false;
    }
    if let Some(container_name) = marker.container_name.as_deref()
        && docker_container_exists(container_name).await
    {
        return false;
    }
    if let Some(process_id) = marker.process_id
        && process_is_live(process_id)
    {
        return false;
    }
    directory.exists()
}

async fn janitor_runtime_directories(runtime_root: &Path) {
    let Ok(entries) = std::fs::read_dir(runtime_root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(marker) = read_storage_marker(&path) else {
            continue;
        };
        if can_remove_marker_owned_directory(&path, &marker).await {
            let _ = tokio::fs::remove_dir_all(&path).await;
        }
    }
}

async fn janitor_qemu_build_directories(runtime_root: &Path) {
    let build_root = qemu_build_root(runtime_root);
    let Ok(entries) = std::fs::read_dir(&build_root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(marker) = read_storage_marker(&path) else {
            continue;
        };
        if can_remove_marker_owned_directory(&path, &marker).await {
            let _ = tokio::fs::remove_dir_all(&path).await;
        }
    }
}

async fn janitor_managed_storage(runtime_root: &Path) {
    janitor_runtime_directories(runtime_root).await;
    janitor_qemu_build_directories(runtime_root).await;
}

fn looks_like_legacy_runtime_directory(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    if Uuid::parse_str(name).is_ok() {
        return true;
    }
    path.join("seed").exists()
        || path.join("data.img").exists()
        || path.join("product-boot.qcow2").exists()
        || path.join("regression-boot.qcow2").exists()
}

fn looks_like_legacy_build_directory(path: &Path) -> bool {
    path.join("boot.qcow2").exists() || path.join("seed.iso").exists()
}

#[derive(serde::Serialize)]
struct ReclaimCandidate {
    path: String,
    tier: String,
    kind: String,
    reason: String,
}

async fn collect_runtime_reclaim_candidates(runtime_root: &Path) -> Vec<ReclaimCandidate> {
    let Ok(entries) = std::fs::read_dir(runtime_root) else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if let Some(marker) = read_storage_marker(&path) {
            if can_remove_marker_owned_directory(&path, &marker).await {
                candidates.push(ReclaimCandidate {
                    path: path.to_string_lossy().to_string(),
                    tier: marker.tier,
                    kind: marker.kind,
                    reason: "marker-owned stale runtime state".to_string(),
                });
            }
            continue;
        }
        if !looks_like_legacy_runtime_directory(&path) {
            continue;
        }
        let name = path.file_name().and_then(|value| value.to_str()).unwrap_or_default();
        let derived_container_name = format!("acu-qemu-{}", name.chars().take(12).collect::<String>());
        if docker_container_exists(&derived_container_name).await {
            continue;
        }
        candidates.push(ReclaimCandidate {
            path: path.to_string_lossy().to_string(),
            tier: "runtime".to_string(),
            kind: "legacy_runtime".to_string(),
            reason: "legacy inspectors runtime directory without an active container reference".to_string(),
        });
    }
    candidates
}

async fn collect_build_reclaim_candidates(runtime_root: &Path) -> Vec<ReclaimCandidate> {
    let build_root = qemu_build_root(runtime_root);
    let Ok(entries) = std::fs::read_dir(&build_root) else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if let Some(marker) = read_storage_marker(&path) {
            if can_remove_marker_owned_directory(&path, &marker).await {
                candidates.push(ReclaimCandidate {
                    path: path.to_string_lossy().to_string(),
                    tier: marker.tier,
                    kind: marker.kind,
                    reason: "marker-owned stale qemu prep workdir".to_string(),
                });
            }
            continue;
        }
        if looks_like_legacy_build_directory(&path) {
            candidates.push(ReclaimCandidate {
                path: path.to_string_lossy().to_string(),
                tier: "runtime".to_string(),
                kind: "legacy_prepare_build".to_string(),
                reason: "legacy qemu prep workdir without ownership marker".to_string(),
            });
        }
    }
    candidates
}

#[derive(serde::Deserialize)]
struct ReclaimStorageRequest {
    mode: Option<String>,
}

async fn reclaim_storage(
    State(state): State<AppState>,
    Json(request): Json<ReclaimStorageRequest>,
) -> Response {
    let apply = request.mode.as_deref() == Some("apply");
    let mut candidates = collect_runtime_reclaim_candidates(&state.artifacts_root).await;
    candidates.extend(collect_build_reclaim_candidates(&state.artifacts_root).await);

    let mut reclaimed = Vec::new();
    if apply {
        for candidate in &candidates {
            remove_runtime_directory(Path::new(&candidate.path)).await;
            reclaimed.push(candidate.path.clone());
        }
    }

    (
        StatusCode::OK,
        Json(json!({
            "mode": if apply { "apply" } else { "report" },
            "runtime_root": state.artifacts_root,
            "cache_root": qemu_cache_root(&state.artifacts_root),
            "exports_root": exports_root(&state.artifacts_root),
            "candidate_count": candidates.len(),
            "candidates": candidates,
            "reclaimed": reclaimed,
        })),
    )
        .into_response()
}

async fn cleanup_orphaned_qemu_containers() {
    if !LinuxBackend::tool_exists("docker") {
        return;
    }

    for prefix in ["acu-qemu-", "acu-image-prep-"] {
        let output = Command::new("docker")
            .args(["ps", "-a", "--format", "{{.Names}}", "--filter", &format!("name={prefix}")])
            .output()
            .await;
        let Ok(output) = output else {
            continue;
        };
        if !output.status.success() {
            continue;
        }

        let names = String::from_utf8_lossy(&output.stdout);
        for container_name in names.lines().map(str::trim).filter(|line| !line.is_empty()) {
            let _ = Command::new("docker")
                .args(["rm", "-f", "-v", container_name])
                .output()
                .await;
        }
    }
}

fn display_session_env(
    display: &str,
    desktop_home: Option<&str>,
    desktop_runtime_dir: Option<&str>,
) -> Vec<(String, String)> {
    if display != ":0" {
        return vec![];
    }

    let mut env = Vec::new();
    let runtime_dir = desktop_runtime_dir
        .map(PathBuf::from)
        .or_else(|| std::env::var("XDG_RUNTIME_DIR").ok().map(PathBuf::from))
        .or_else(|| {
            Path::new(QEMU_PRODUCT_RUNTIME_DIR)
                .exists()
                .then(|| PathBuf::from(QEMU_PRODUCT_RUNTIME_DIR))
        });
    if let Some(runtime_dir) = runtime_dir.as_ref()
        && runtime_dir.exists()
    {
        env.push((
            "XDG_RUNTIME_DIR".to_string(),
            runtime_dir.to_string_lossy().to_string(),
        ));
        let session_bus = runtime_dir.join("bus");
        if session_bus.exists() {
            env.push((
                "DBUS_SESSION_BUS_ADDRESS".to_string(),
                format!("unix:path={}", session_bus.to_string_lossy()),
            ));
        }
    }

    if let Ok(xauthority) = std::env::var("XAUTHORITY")
        && Path::new(&xauthority).exists()
    {
        env.push(("XAUTHORITY".to_string(), xauthority));
        return env;
    }

    let mut candidates = Vec::new();
    if let Some(runtime_dir) = runtime_dir.as_ref() {
        candidates.push(runtime_dir.join("gdm/Xauthority"));
        candidates.push(runtime_dir.join("Xauthority"));
    }
    if let Some(desktop_home) = desktop_home {
        candidates.push(Path::new(desktop_home).join(".Xauthority"));
    }

    for candidate in candidates {
        if candidate.exists() {
            env.push((
                "XAUTHORITY".to_string(),
                candidate.to_string_lossy().to_string(),
            ));
            break;
        }
    }

    env
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
        Ok(session) => (
            StatusCode::CREATED,
            Json(json!({ "session": enrich_session_record(&session) })),
        )
            .into_response(),
        Err((status, error)) => (status, Json(json!({ "error": error }))).into_response(),
    }
}

async fn list_sessions(State(state): State<AppState>) -> Response {
    let sessions = state
        .sessions
        .lock()
        .await
        .values()
        .map(|handle| enrich_session_record(&handle.record))
        .collect::<Vec<_>>();
    (StatusCode::OK, Json(json!({ "sessions": sessions }))).into_response()
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

async fn create_session_artifacts_dir(
    state: &AppState,
    session_id: &str,
) -> Result<PathBuf, (StatusCode, Value)> {
    let artifacts_dir = state.artifacts_root.join(session_id);
    tokio::fs::create_dir_all(&artifacts_dir)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({ "code": "artifacts_dir_failed", "message": error.to_string() }),
            )
        })?;
    std::fs::canonicalize(&artifacts_dir).map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "code": "artifacts_dir_canonicalize_failed", "message": error.to_string() }),
        )
    })
}

async fn mark_runtime_session_directory(
    directory: &Path,
    session_id: &str,
    provider: &str,
    qemu_profile: Option<&str>,
    container_name: Option<&str>,
    process_id: Option<u32>,
) -> Result<(), (StatusCode, Value)> {
    let marker = StorageOwnershipMarker::runtime_session(
        session_id,
        provider,
        qemu_profile,
        container_name,
        process_id,
    );
    write_storage_marker(directory, &marker).await.map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "code": "artifacts_marker_failed", "message": error.to_string() }),
        )
    })
}

async fn remove_runtime_directory(path: &Path) {
    let _ = tokio::fs::remove_dir_all(path).await;
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
    let artifacts_dir = create_session_artifacts_dir(state, &session_id).await?;

    let screen_geometry = format!("{}x{}x24", request.width, request.height);
    let mut selected = None;
    let mut last_status = None;
    for display in candidate_displays(state).await {
        let mut child = Command::new("Xvfb")
            .arg(&display)
            .args(["-screen", "0", &screen_geometry, "-nolisten", "tcp", "-ac"])
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
            last_status = Some((display, status.to_string()));
            continue;
        }

        selected = Some((display, child));
        break;
    }

    let (display, child) = if let Some(selected) = selected {
        selected
    } else {
        remove_runtime_directory(&artifacts_dir).await;
        let message = if let Some((display, status)) = last_status {
            format!("Xvfb exited early on {display}: {status}")
        } else {
            "Xvfb could not find an available display".to_string()
        };
        return Err((
            StatusCode::FAILED_DEPENDENCY,
            json!({ "code": "xvfb_early_exit", "message": message }),
        ));
    };
    let process_id = child.id();
    if let Err(error) =
        mark_runtime_session_directory(&artifacts_dir, &session_id, "xvfb", None, None, process_id)
            .await
    {
        let mut child = child;
        let _ = child.kill().await;
        remove_runtime_directory(&artifacts_dir).await;
        return Err(error);
    }

    let backend = LinuxBackend::new(BackendOptions {
        display: display.clone(),
        artifacts_dir: artifacts_dir.clone(),
        browser_command: state.browser_command.clone(),
        session_env: vec![],
        default_user: None,
        default_user_home: None,
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
        desktop_user: None,
        desktop_home: None,
        desktop_runtime_dir: None,
        runtime_base_url: Some(state.runtime_base_url.clone()),
        viewer_url: None,
        live_desktop_view: None,
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
    let default_user = request.desktop_user.clone();
    let desktop_home = request
        .desktop_home
        .clone()
        .or_else(|| std::env::var("HOME").ok());
    let desktop_runtime_dir = request
        .desktop_runtime_dir
        .clone()
        .or_else(|| std::env::var("XDG_RUNTIME_DIR").ok());
    let artifacts_dir = create_session_artifacts_dir(state, &session_id).await?;
    mark_runtime_session_directory(&artifacts_dir, &session_id, "display", None, None, None)
        .await?;

    let backend = LinuxBackend::new(BackendOptions {
        display: display.clone(),
        artifacts_dir: artifacts_dir.clone(),
        browser_command: browser_command.clone(),
        session_env: display_session_env(
            &display,
            desktop_home.as_deref(),
            desktop_runtime_dir.as_deref(),
        ),
        default_user: default_user.clone(),
        default_user_home: desktop_home.clone(),
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
        desktop_user: request.desktop_user.clone(),
        desktop_home,
        desktop_runtime_dir,
        runtime_base_url: Some(state.runtime_base_url.clone()),
        viewer_url: None,
        live_desktop_view: None,
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
    let guest_runtime_binary_path = std::env::current_exe().map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "code": "guest_runtime_binary_unavailable", "message": error.to_string() }),
        )
    })?;
    let artifacts_dir = create_session_artifacts_dir(state, &session_id).await?;
    let absolute_artifacts_dir = artifacts_dir.clone();

    let container_name = format!("acu-qemu-{}", &session_id[..12]);
    mark_runtime_session_directory(
        &absolute_artifacts_dir,
        &session_id,
        "qemu",
        Some(qemu_profile.as_str()),
        Some(&container_name),
        None,
    )
    .await?;
    let image = request
        .container_image
        .clone()
        .or_else(|| std::env::var("ACU_QEMU_CONTAINER_IMAGE").ok())
        .unwrap_or_else(|| "qemux/qemu".to_string());
    let mut boot = request
        .boot
        .clone()
        .or_else(|| std::env::var("ACU_QEMU_BOOT").ok())
        .unwrap_or_else(|| "alpine".to_string());
    let mut boot_image_path = None;
    let mut seed_iso_path = None;
    if request.boot.is_none() {
        let template_image = ensure_qemu_profile_image(state, qemu_profile).await?;
        let session_boot_image =
            absolute_artifacts_dir.join(format!("{}-boot.qcow2", qemu_profile.as_str()));
        tokio::fs::copy(&template_image, &session_boot_image)
            .await
            .map_err(|error| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    json!({ "code": "qemu_image_copy_failed", "message": error.to_string() }),
                )
            })?;
        boot = "/boot.qcow2".to_string();
        boot_image_path = Some(session_boot_image);
        seed_iso_path = Some(
            build_qemu_session_seed_iso(
                &absolute_artifacts_dir,
                &session_id,
                qemu_profile,
                &image,
                state.qemu_guest_runtime_port,
                &state.browser_command,
            )
            .await?,
        );
    }
    let shared_host_path = if let Some(shared_host_path) = request.shared_host_path.as_deref() {
        Some(std::fs::canonicalize(shared_host_path).map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                json!({
                    "code": "shared_host_path_invalid",
                    "message": error.to_string(),
                    "path": shared_host_path,
                }),
            )
        })?)
    } else {
        None
    };
    let disable_kvm = request
        .disable_kvm
        .unwrap_or_else(|| !Path::new("/dev/kvm").exists());

    let container_spec = QemuContainerSpec {
        container_name: &container_name,
        image: &image,
        boot: &boot,
        artifacts_dir: &absolute_artifacts_dir,
        guest_runtime_binary_path: &guest_runtime_binary_path,
        boot_image_path: boot_image_path.as_deref(),
        seed_iso_path: seed_iso_path.as_deref(),
        shared_host_path: shared_host_path.as_deref(),
        viewer_port: state.qemu_viewer_port,
        runtime_port: state.qemu_guest_runtime_port,
        disable_kvm,
    };
    let launch_mode = match launch_qemu_container(&container_spec).await {
        Ok(mode) => mode,
        Err(message) => {
            remove_runtime_directory(&absolute_artifacts_dir).await;
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
        let _ = docker_output(&["rm", "-f", "-v", &container_name]).await;
        remove_runtime_directory(&absolute_artifacts_dir).await;
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
    let viewer_url = if let Some(viewer_url) =
        resolve_qemu_endpoint(viewer_port, &container_ip, state.qemu_viewer_port)
    {
        viewer_url
    } else {
        let _ = docker_output(&["rm", "-f", "-v", &container_name]).await;
        remove_runtime_directory(&absolute_artifacts_dir).await;
        return Err((
            StatusCode::FAILED_DEPENDENCY,
            json!({
                "code": "qemu_container_ip_missing",
                "message": "qemu container started but did not expose a viewer port or bridge-network IP",
            }),
        ));
    };
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
    let desktop_user = (qemu_profile == QemuSessionProfile::Product)
        .then(|| QEMU_PRODUCT_DESKTOP_USER.to_string());
    let desktop_home = desktop_user
        .as_ref()
        .map(|_| QEMU_PRODUCT_DESKTOP_HOME.to_string());
    let desktop_runtime_dir = desktop_user
        .as_ref()
        .map(|_| QEMU_PRODUCT_RUNTIME_DIR.to_string());
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
        desktop_user,
        desktop_home,
        desktop_runtime_dir,
        runtime_base_url: None,
        viewer_url: Some(viewer_url.clone()),
        live_desktop_view: None,
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
        let bridge_timeout = match qemu_profile {
            QemuSessionProfile::Product => {
                std::cmp::max(state.qemu_bridge_probe_timeout, Duration::from_secs(180))
            }
            QemuSessionProfile::Regression => {
                std::cmp::max(state.qemu_bridge_probe_timeout, Duration::from_secs(90))
            }
        };
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
            timeout: bridge_timeout,
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
                    desktop_user: (monitor.qemu_profile == QemuSessionProfile::Product)
                        .then(|| QEMU_PRODUCT_DESKTOP_USER.to_string()),
                    desktop_home: (monitor.qemu_profile == QemuSessionProfile::Product)
                        .then(|| QEMU_PRODUCT_DESKTOP_HOME.to_string()),
                    desktop_runtime_dir: (monitor.qemu_profile == QemuSessionProfile::Product)
                        .then(|| QEMU_PRODUCT_RUNTIME_DIR.to_string()),
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

    let mut failed_container = None;
    let mut guard = monitor.sessions.lock().await;
    if let Some(handle) = guard.get_mut(&monitor.session_id) {
        handle.record.bridge_status = Some("failed".to_string());
        promote_readiness(&mut handle.record, "failed");
        handle.record.bridge_error = Some(bridge_error);
        handle.record.viewer_url = None;
        handle.record.runtime_base_url = None;
        if let SessionProviderHandle::QemuDocker { container_name } = &handle.provider_handle {
            failed_container = Some(container_name.clone());
        }
    }
    drop(guard);
    if let Some(container_name) = failed_container {
        let _ = docker_output(&["rm", "-f", &container_name]).await;
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

async fn ensure_qemu_profile_image(
    state: &AppState,
    profile: QemuSessionProfile,
) -> Result<PathBuf, (StatusCode, Value)> {
    let cache_root = qemu_cache_root(&state.artifacts_root);
    tokio::fs::create_dir_all(&cache_root)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({ "code": "qemu_cache_dir_failed", "message": error.to_string() }),
            )
        })?;
    let script_path = std::env::var("ACU_QEMU_ASSET_SCRIPT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("scripts")
                .join("qemu_guest_assets.py")
        });
    let guest_runtime_binary = std::env::current_exe().map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "code": "guest_runtime_exe_missing", "message": error.to_string() }),
        )
    })?;
    let qemu_container_image =
        std::env::var("ACU_QEMU_CONTAINER_IMAGE").unwrap_or_else(|_| "qemux/qemu".to_string());
    let output = Command::new("python3")
        .args([
            script_path.to_string_lossy().as_ref(),
            "ensure-image",
            "--profile",
            profile.as_str(),
            "--cache-root",
            cache_root.to_string_lossy().as_ref(),
            "--guest-runtime-binary",
            guest_runtime_binary.to_string_lossy().as_ref(),
            "--qemu-image",
            &qemu_container_image,
            "--browser-command",
            &state.browser_command,
        ])
        .output()
        .await
        .map_err(|error| {
            (
                StatusCode::FAILED_DEPENDENCY,
                json!({ "code": "qemu_asset_builder_failed", "message": error.to_string() }),
            )
        })?;
    if !output.status.success() {
        return Err((
            StatusCode::FAILED_DEPENDENCY,
            json!({
                "code": "qemu_asset_builder_failed",
                "status": output.status.code(),
                "stderr": String::from_utf8_lossy(&output.stderr),
            }),
        ));
    }
    let ensured: EnsuredQemuImage = serde_json::from_slice(&output.stdout).map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({
                "code": "qemu_asset_builder_decode_failed",
                "message": error.to_string(),
                "stdout": String::from_utf8_lossy(&output.stdout),
            }),
        )
    })?;
    Ok(PathBuf::from(ensured.image_path))
}

async fn build_qemu_session_seed_iso(
    artifacts_dir: &Path,
    session_id: &str,
    profile: QemuSessionProfile,
    qemu_image: &str,
    runtime_port: u16,
    browser_command: &str,
) -> Result<PathBuf, (StatusCode, Value)> {
    let seed_dir = artifacts_dir.join("seed");
    tokio::fs::create_dir_all(&seed_dir)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({ "code": "qemu_seed_dir_failed", "message": error.to_string() }),
            )
        })?;
    let user_data = match profile {
        QemuSessionProfile::Regression => regression_seed_user_data(runtime_port, browser_command),
        QemuSessionProfile::Product => product_seed_user_data(runtime_port, browser_command),
    };
    let meta_data = format!(
        "instance-id: acu-session-{session_id}\nlocal-hostname: acu-{}\n",
        profile.as_str()
    );
    tokio::fs::write(seed_dir.join("user-data"), user_data)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({ "code": "qemu_seed_write_failed", "message": error.to_string() }),
            )
        })?;
    tokio::fs::write(seed_dir.join("meta-data"), meta_data)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({ "code": "qemu_seed_write_failed", "message": error.to_string() }),
            )
        })?;
    let output = Command::new("docker")
        .args([
            "run",
            "--rm",
            "-v",
            &format!("{}:/work", seed_dir.to_string_lossy()),
            "--entrypoint",
            "sh",
            qemu_image,
            "-lc",
            "cd /work && genisoimage -output /work/seed.iso -volid cidata -joliet -rock user-data meta-data >/dev/null 2>&1",
        ])
        .output()
        .await
        .map_err(|error| {
            (
                StatusCode::FAILED_DEPENDENCY,
                json!({ "code": "qemu_seed_iso_failed", "message": error.to_string() }),
            )
        })?;
    if !output.status.success() {
        return Err((
            StatusCode::FAILED_DEPENDENCY,
            json!({
                "code": "qemu_seed_iso_failed",
                "stderr": String::from_utf8_lossy(&output.stderr),
            }),
        ));
    }
    Ok(seed_dir.join("seed.iso"))
}

fn regression_seed_user_data(runtime_port: u16, browser_command: &str) -> String {
    format!(
        r#"#cloud-config
packages:
  - xdotool
  - x11-utils
  - x11-apps
  - x11-xserver-utils
  - xinit
  - xvfb
  - imagemagick
  - curl
write_files:
  - path: /etc/systemd/system/acu-guest-runtime.service
    permissions: '0644'
    content: |
      [Unit]
      Description=ACU Guest Runtime
      After=network-online.target
      Wants=network-online.target

      [Service]
      ExecStart=/usr/local/bin/acu-guest-runtime --host 0.0.0.0 --port {runtime_port} --browser-command {browser_command}
      Restart=always
      RestartSec=2
      StandardOutput=journal+console
      StandardError=journal+console

      [Install]
      WantedBy=multi-user.target
runcmd:
  - [ bash, -lc, 'systemctl disable --now ufw || true' ]
  - [ bash, -lc, 'echo regression > /var/lib/acu-session-profile' ]
  - [ bash, -lc, 'modprobe 9pnet_virtio || true; modprobe 9p || true; mkdir -p /mnt/shared; mount -t 9p -o trans=virtio shared /mnt/shared || true' ]
  - [ bash, -lc, 'install -m 0755 /mnt/shared/guest-runtime /usr/local/bin/acu-guest-runtime' ]
  - [ bash, -lc, 'systemctl daemon-reload && systemctl enable acu-guest-runtime.service && systemctl restart acu-guest-runtime.service' ]
 "#,
    )
}

fn product_seed_user_data(runtime_port: u16, browser_command: &str) -> String {
    format!(
        r#"#cloud-config
write_files:
  - path: /etc/gdm3/custom.conf
    permissions: '0644'
    content: |
      [daemon]
      WaylandEnable=false
      AutomaticLoginEnable=true
      AutomaticLogin=ubuntu
  - path: /etc/systemd/system/acu-guest-runtime.service
    permissions: '0644'
    content: |
      [Unit]
      Description=ACU Guest Runtime
      After=network-online.target gdm.service
      Wants=network-online.target

      [Service]
      Environment=PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
      ExecStart=/usr/local/bin/acu-guest-runtime --host 0.0.0.0 --port {runtime_port} --browser-command {browser_command}
      Restart=always
      RestartSec=2
      StandardOutput=journal+console
      StandardError=journal+console

      [Install]
      WantedBy=multi-user.target
runcmd:
  - [ bash, -lc, 'systemctl disable --now ufw || true' ]
  - [ bash, -lc, 'echo product > /var/lib/acu-session-profile' ]
  - [ bash, -lc, 'modprobe 9pnet_virtio || true; modprobe 9p || true; mkdir -p /mnt/shared; mount -t 9p -o trans=virtio shared /mnt/shared || true' ]
  - [ bash, -lc, 'install -m 0755 /mnt/shared/guest-runtime /usr/local/bin/acu-guest-runtime' ]
  - [ bash, -lc, 'ln -sf /usr/bin/epiphany-browser /usr/local/bin/firefox || true' ]
  - [ bash, -lc, 'systemctl daemon-reload && systemctl enable acu-guest-runtime.service && systemctl restart acu-guest-runtime.service' ]
  - [ bash, -lc, 'systemctl set-default graphical.target || true' ]
"#,
    )
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
    let args = docker_run_args(spec, launch_mode);
    Command::new("docker")
        .args(&args)
        .output()
        .await
        .map_err(|error| error.to_string())
}

fn docker_run_args(spec: &QemuContainerSpec<'_>, launch_mode: QemuLaunchMode) -> Vec<String> {
    let boot_value = if spec.boot_image_path.is_some() {
        "/boot.qcow2"
    } else {
        spec.boot
    };
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
    args.push(format!("BOOT={boot_value}"));
    args.push("-e".to_string());
    args.push(format!("USER_PORTS=22,{}", spec.runtime_port));
    args.push("-v".to_string());
    args.push(format!("{}:/storage", spec.artifacts_dir.to_string_lossy()));
    if let Some(boot_image_path) = spec.boot_image_path {
        args.push("-v".to_string());
        args.push(format!("{}:/boot.qcow2", boot_image_path.to_string_lossy()));
    }
    if let Some(seed_iso_path) = spec.seed_iso_path {
        args.push("-v".to_string());
        args.push(format!("{}:/seed.iso:ro", seed_iso_path.to_string_lossy()));
        args.push("-e".to_string());
        args.push("ARGUMENTS=-drive file=/seed.iso,format=raw,media=cdrom,readonly=on".to_string());
    }
    if let Some(shared_host_path) = spec.shared_host_path {
        args.push("-v".to_string());
        args.push(format!(
            "{}:/shared/hostshare:ro",
            shared_host_path.to_string_lossy()
        ));
    }
    args.push("-v".to_string());
    args.push(format!(
        "{}:/shared/guest-runtime:ro",
        spec.guest_runtime_binary_path.to_string_lossy()
    ));
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
    args
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
        Some(session) => (
            StatusCode::OK,
            Json(json!({ "session": enrich_session_record(&session) })),
        )
            .into_response(),
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
            let artifacts_dir = PathBuf::from(&handle.record.artifacts_dir);
            if let Some(remote_bridge) = handle.remote_bridge.as_ref() {
                if let Some(remote_session_id) = remote_bridge.session_id.as_ref() {
                    let _ = state
                        .http_client
                        .delete(format!(
                            "{}/api/sessions/{}",
                            remote_bridge.base_url, remote_session_id
                        ))
                        .send()
                        .await;
                }
            }
            match &mut handle.provider_handle {
                SessionProviderHandle::Xvfb { child } => {
                    let _ = child.kill().await;
                }
                SessionProviderHandle::ExistingDisplay => {}
                SessionProviderHandle::QemuDocker { container_name } => {
                    let _ = docker_output(&["rm", "-f", "-v", container_name]).await;
                }
            }
            remove_runtime_directory(&artifacts_dir).await;
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
            "live_desktop_view": derive_live_desktop_view(record),
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

async fn candidate_displays(state: &AppState) -> Vec<String> {
    let sessions = state.sessions.lock().await;
    let in_use: BTreeSet<u16> = sessions
        .values()
        .filter_map(|handle| handle.record.display.as_deref())
        .filter_map(|display| display.strip_prefix(':'))
        .filter_map(|display| display.parse::<u16>().ok())
        .collect();

    let mut displays = Vec::new();
    for candidate in 90..140 {
        if !in_use.contains(&candidate) {
            displays.push(format!(":{candidate}"));
        }
    }
    let mut candidate = 140;
    while displays.len() < 20 {
        if !in_use.contains(&candidate) {
            displays.push(format!(":{candidate}"));
        }
        candidate += 1;
    }
    displays
}

#[cfg(test)]
mod tests {
    use super::{
        QEMU_PRODUCT_DESKTOP_HOME, QEMU_PRODUCT_DESKTOP_USER, QEMU_PRODUCT_RUNTIME_DIR,
        QemuContainerSpec, QemuLaunchMode, SessionRecord, derive_live_desktop_view,
        docker_run_args, enrich_session_record, merge_capabilities, parse_published_port,
        resolve_qemu_endpoint, should_retry_qemu_without_published_ports,
    };
    use chrono::Utc;
    use std::path::Path;

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

    #[test]
    fn docker_run_args_include_seed_iso_and_shared_path_mounts() {
        let spec = QemuContainerSpec {
            container_name: "acu-test",
            image: "qemux/qemu",
            boot: "/boot.qcow2",
            artifacts_dir: Path::new("/tmp/artifacts"),
            guest_runtime_binary_path: Path::new("/tmp/guest-runtime"),
            boot_image_path: Some(Path::new("/tmp/boot.qcow2")),
            seed_iso_path: Some(Path::new("/tmp/seed.iso")),
            shared_host_path: Some(Path::new("/tmp/shared")),
            viewer_port: 8006,
            runtime_port: 4001,
            disable_kvm: true,
        };
        let args = docker_run_args(&spec, QemuLaunchMode::BridgeNetwork);
        assert!(args.iter().any(|arg| arg == "BOOT=/boot.qcow2"));
        assert!(
            args.iter()
                .any(|arg| arg.contains("/tmp/boot.qcow2:/boot.qcow2"))
        );
        assert!(
            args.iter()
                .any(|arg| arg.contains("/tmp/seed.iso:/seed.iso:ro"))
        );
        assert!(args.iter().any(|arg| {
            arg == "ARGUMENTS=-drive file=/seed.iso,format=raw,media=cdrom,readonly=on"
        }));
        assert!(
            args.iter()
                .any(|arg| arg.contains("/tmp/shared:/shared/hostshare:ro"))
        );
        assert!(
            args.iter()
                .any(|arg| arg.contains("/tmp/guest-runtime:/shared/guest-runtime:ro"))
        );
    }

    #[test]
    fn product_seed_user_data_reinstalls_guest_runtime_and_enables_service() {
        let user_data = super::product_seed_user_data(4900, "firefox");
        assert!(user_data.contains(
            "install -m 0755 /mnt/shared/guest-runtime /usr/local/bin/acu-guest-runtime"
        ));
        assert!(user_data.contains("/etc/systemd/system/acu-guest-runtime.service"));
        assert!(user_data.contains("AutomaticLoginEnable=true"));
        assert!(user_data.contains("systemctl set-default graphical.target"));
        assert!(user_data.contains("systemctl enable acu-guest-runtime.service"));
        assert!(user_data.contains("acu-guest-runtime --host 0.0.0.0 --port 4900"));
    }

    #[test]
    fn regression_seed_user_data_reinstalls_guest_runtime_and_enables_service() {
        let user_data = super::regression_seed_user_data(4900, "firefox");
        assert!(user_data.contains(
            "install -m 0755 /mnt/shared/guest-runtime /usr/local/bin/acu-guest-runtime"
        ));
        assert!(user_data.contains("systemctl enable acu-guest-runtime.service"));
        assert!(user_data.contains("echo regression > /var/lib/acu-session-profile"));
        assert!(user_data.contains("acu-guest-runtime --host 0.0.0.0 --port 4900"));
    }

    #[test]
    fn derives_truthful_live_desktop_view_modes() {
        let qemu_product = SessionRecord {
            id: "qemu-product".to_string(),
            provider: "qemu".to_string(),
            qemu_profile: Some("product".to_string()),
            display: None,
            width: 1440,
            height: 900,
            state: "running".to_string(),
            created_at: Utc::now(),
            artifacts_dir: "artifacts/runtime/qemu-product".to_string(),
            capabilities: vec!["viewer".to_string()],
            browser_command: Some("firefox".to_string()),
            desktop_user: Some(QEMU_PRODUCT_DESKTOP_USER.to_string()),
            desktop_home: Some(QEMU_PRODUCT_DESKTOP_HOME.to_string()),
            desktop_runtime_dir: Some(QEMU_PRODUCT_RUNTIME_DIR.to_string()),
            runtime_base_url: None,
            viewer_url: Some("http://127.0.0.1:32771".to_string()),
            live_desktop_view: None,
            bridge_status: Some("runtime_ready".to_string()),
            readiness_state: Some("runtime_ready".to_string()),
            bridge_error: None,
        };
        let qemu_regression = SessionRecord {
            qemu_profile: Some("regression".to_string()),
            ..qemu_product.clone()
        };
        let xvfb = SessionRecord {
            id: "xvfb".to_string(),
            provider: "xvfb".to_string(),
            qemu_profile: None,
            display: Some(":90".to_string()),
            viewer_url: None,
            ..qemu_product.clone()
        };

        let qemu_product_view = derive_live_desktop_view(&qemu_product);
        assert_eq!(qemu_product_view.mode, "stream");
        assert_eq!(qemu_product_view.provider_surface, "qemu_novnc");
        assert!(qemu_product_view.matches_action_plane);

        let qemu_regression_view = derive_live_desktop_view(&qemu_regression);
        assert_eq!(qemu_regression_view.mode, "screenshot_poll");
        assert_eq!(
            qemu_regression_view.provider_surface,
            "guest_xvfb_screenshot"
        );
        assert!(qemu_regression_view.debug_url.is_some());

        let xvfb_view = derive_live_desktop_view(&xvfb);
        assert_eq!(xvfb_view.mode, "screenshot_poll");
        assert_eq!(xvfb_view.status, "ready");
        assert!(xvfb_view.reason.is_some());
    }

    #[test]
    fn enriches_session_record_with_live_desktop_view() {
        let record = SessionRecord {
            id: "qemu-product".to_string(),
            provider: "qemu".to_string(),
            qemu_profile: Some("product".to_string()),
            display: None,
            width: 1440,
            height: 900,
            state: "running".to_string(),
            created_at: Utc::now(),
            artifacts_dir: "artifacts/runtime/qemu-product".to_string(),
            capabilities: vec!["viewer".to_string()],
            browser_command: Some("firefox".to_string()),
            desktop_user: Some(QEMU_PRODUCT_DESKTOP_USER.to_string()),
            desktop_home: Some(QEMU_PRODUCT_DESKTOP_HOME.to_string()),
            desktop_runtime_dir: Some(QEMU_PRODUCT_RUNTIME_DIR.to_string()),
            runtime_base_url: None,
            viewer_url: Some("http://127.0.0.1:32771".to_string()),
            live_desktop_view: None,
            bridge_status: Some("runtime_ready".to_string()),
            readiness_state: Some("runtime_ready".to_string()),
            bridge_error: None,
        };

        let enriched = enrich_session_record(&record);
        assert!(enriched.live_desktop_view.is_some());
        assert_eq!(record.live_desktop_view, None);
    }
}
