use std::env;
use std::fs::{self, File};
use std::io;
use std::net::TcpListener;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use guest_runtime::RuntimeConfig;
use tauri::{AppHandle, Manager, RunEvent, WebviewWindow, async_runtime};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

include!(concat!(env!("OUT_DIR"), "/embedded_assets.rs"));

const WINDOW_LABEL: &str = "main";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(250);
const DEFAULT_BROWSER_COMMAND: &str = "firefox";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ActivationRequest {
    activate_desktop: bool,
    session_id: Option<String>,
}

struct SupervisorHandle {
    control_plane_url: String,
    control_plane_child: Arc<Mutex<Option<Child>>>,
}

struct DesktopRuntimeState {
    supervisor: Mutex<Option<SupervisorHandle>>,
    pending_session: Mutex<Option<String>>,
    control_plane_url: Mutex<Option<String>>,
}

impl Default for DesktopRuntimeState {
    fn default() -> Self {
        Self {
            supervisor: Mutex::new(None),
            pending_session: Mutex::new(None),
            control_plane_url: Mutex::new(None),
        }
    }
}

pub fn run() {
    let initial_activation = parse_activation_request(env::args().skip(1));
    let state = DesktopRuntimeState::default();
    let mut builder = tauri::Builder::default().manage(state);

    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            let activation = parse_activation_request(args);
            let handle = app.clone();
            async_runtime::spawn(async move {
                if let Err(error) = apply_activation(handle, activation).await {
                    eprintln!("desktop activation failed: {error}");
                }
            });
        }));
    }

    builder
        .setup(move |app| {
            let app_handle = app.handle().clone();
            let startup_activation = initial_activation.clone();
            async_runtime::spawn(async move {
                if let Err(error) = bootstrap_desktop_app(app_handle, startup_activation).await {
                    eprintln!("desktop bootstrap failed: {error}");
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("build desktop app")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                let handle = app.clone();
                async_runtime::spawn(async move {
                    shutdown_control_plane(handle).await;
                });
            }
        });
}

async fn bootstrap_desktop_app(
    app: AppHandle,
    activation: ActivationRequest,
) -> Result<(), String> {
    let session_id = activation.session_id.clone();
    if let Some(session_id) = session_id {
        store_pending_session(&app, Some(session_id)).await;
    }

    let control_plane_url = ensure_supervisor(&app).await?;
    let session_id = take_pending_session(&app).await;
    navigate_main_window(&app, &control_plane_url, session_id.as_deref())?;
    Ok(())
}

async fn apply_activation(app: AppHandle, activation: ActivationRequest) -> Result<(), String> {
    if let Some(session_id) = activation.session_id {
        store_pending_session(&app, Some(session_id)).await;
    }

    let control_plane_url = ensure_supervisor(&app).await?;
    let pending_session = take_pending_session(&app).await;
    navigate_main_window(&app, &control_plane_url, pending_session.as_deref())?;
    Ok(())
}

async fn ensure_supervisor(app: &AppHandle) -> Result<String, String> {
    {
        let state = app.state::<DesktopRuntimeState>();
        let supervisor = state.supervisor.lock().await;
        if let Some(handle) = supervisor.as_ref() {
            return Ok(handle.control_plane_url.clone());
        }
    }

    let supervisor = start_supervisor(app).await?;
    let control_plane_url = supervisor.control_plane_url.clone();
    let state = app.state::<DesktopRuntimeState>();
    *state.control_plane_url.lock().await = Some(control_plane_url.clone());
    *state.supervisor.lock().await = Some(supervisor);
    Ok(control_plane_url)
}

async fn start_supervisor(app: &AppHandle) -> Result<SupervisorHandle, String> {
    let app_data_dir = app
        .path()
        .app_local_data_dir()
        .or_else(|_| app.path().app_data_dir())
        .map_err(|_| "desktop app data directory unavailable".to_string())?;
    fs::create_dir_all(&app_data_dir).map_err(|error| error.to_string())?;

    let control_plane_root = app_data_dir.join("control-plane-root");
    let artifact_root = app_data_dir.join("artifacts");
    let logs_root = app_data_dir.join("logs");
    fs::create_dir_all(&artifact_root).map_err(|error| error.to_string())?;
    fs::create_dir_all(&logs_root).map_err(|error| error.to_string())?;
    write_embedded_assets(&control_plane_root).map_err(|error| error.to_string())?;

    let guest_runtime_port = next_available_port().map_err(|error| error.to_string())?;
    let control_plane_port = next_available_port().map_err(|error| error.to_string())?;
    let guest_runtime_url = format!("http://127.0.0.1:{guest_runtime_port}");
    let control_plane_url = format!("http://127.0.0.1:{control_plane_port}");

    let mut guest_runtime_config = RuntimeConfig::from_env_and_args();
    guest_runtime_config.bind_host = "127.0.0.1".to_string();
    guest_runtime_config.port = guest_runtime_port;
    guest_runtime_config.artifacts_root = artifact_root.join("runtime");
    guest_runtime_config.browser_command =
        env::var("ACU_BROWSER_COMMAND").unwrap_or_else(|_| DEFAULT_BROWSER_COMMAND.to_string());
    fs::create_dir_all(&guest_runtime_config.artifacts_root).map_err(|error| error.to_string())?;

    async_runtime::spawn(async move {
        guest_runtime::run(guest_runtime_config).await;
    });
    wait_for_health(&guest_runtime_url, "/health").await?;

    let control_plane_child = spawn_control_plane(
        &control_plane_root,
        &artifact_root,
        &logs_root,
        &guest_runtime_url,
        control_plane_port,
    )
    .await?;
    wait_for_health(&control_plane_url, "/api/health").await?;

    Ok(SupervisorHandle {
        control_plane_url,
        control_plane_child: Arc::new(Mutex::new(Some(control_plane_child))),
    })
}

async fn spawn_control_plane(
    control_plane_root: &Path,
    artifact_root: &Path,
    logs_root: &Path,
    guest_runtime_url: &str,
    control_plane_port: u16,
) -> Result<Child, String> {
    let node_bin = env::var("ACU_NODE_BIN").unwrap_or_else(|_| "node".to_string());
    let stdout_log =
        File::create(logs_root.join("control-plane.log")).map_err(|error| error.to_string())?;
    let stderr_log =
        File::create(logs_root.join("control-plane.err.log")).map_err(|error| error.to_string())?;
    let current_exe = env::current_exe().map_err(|error| error.to_string())?;
    let ui_root = control_plane_root.join("ui");

    let mut command = Command::new(node_bin);
    command
        .arg(control_plane_root.join("dist/index.js"))
        .current_dir(control_plane_root)
        .env("PORT", control_plane_port.to_string())
        .env("GUEST_RUNTIME_URL", guest_runtime_url)
        .env("ACU_UI_ROOT", &ui_root)
        .env("ACU_ARTIFACT_ROOT", artifact_root)
        .env("ACU_DESKTOP_ACTIVATE_BIN", current_exe)
        .env("ACU_DESKTOP_ACTIVATE_ARGS_JSON", "[]")
        .stdout(Stdio::from(stdout_log))
        .stderr(Stdio::from(stderr_log))
        .kill_on_drop(true);

    if let Ok(playwright_enabled) = env::var("ACU_ENABLE_PLAYWRIGHT") {
        command.env("ACU_ENABLE_PLAYWRIGHT", playwright_enabled);
    }
    if let Ok(browser_backend) = env::var("ACU_BROWSER_BACKEND") {
        command.env("ACU_BROWSER_BACKEND", browser_backend);
    }
    if let Ok(firefox_executable) = env::var("FIREFOX_EXECUTABLE") {
        command.env("FIREFOX_EXECUTABLE", firefox_executable);
    }

    command
        .spawn()
        .map_err(|error| format!("spawn control-plane: {error}"))
}

async fn wait_for_health(base_url: &str, path: &str) -> Result<(), String> {
    let deadline = std::time::Instant::now() + STARTUP_TIMEOUT;
    let client = reqwest::Client::new();
    let url = format!("{base_url}{path}");
    loop {
        match client.get(&url).send().await {
            Ok(response) if response.status().is_success() => return Ok(()),
            _ => {
                if std::time::Instant::now() >= deadline {
                    return Err(format!("timed out waiting for {url}"));
                }
                tokio::time::sleep(HEALTH_POLL_INTERVAL).await;
            }
        }
    }
}

fn navigate_main_window(
    app: &AppHandle,
    control_plane_url: &str,
    session_id: Option<&str>,
) -> Result<(), String> {
    let window = app
        .get_webview_window(WINDOW_LABEL)
        .ok_or_else(|| "main window missing".to_string())?;
    let target = build_operator_url(control_plane_url, session_id);
    window
        .eval(format!("window.location.replace({target:?});"))
        .map_err(|error| error.to_string())?;
    focus_window(&window).map_err(|error| error.to_string())?;
    Ok(())
}

fn focus_window(window: &WebviewWindow) -> tauri::Result<()> {
    window.show()?;
    window.unminimize()?;
    window.set_focus()?;
    Ok(())
}

fn build_operator_url(base_url: &str, session_id: Option<&str>) -> String {
    match session_id {
        Some(session_id) => format!("{base_url}/?session={session_id}"),
        None => format!("{base_url}/"),
    }
}

async fn store_pending_session(app: &AppHandle, session_id: Option<String>) {
    let state = app.state::<DesktopRuntimeState>();
    *state.pending_session.lock().await = session_id;
}

async fn take_pending_session(app: &AppHandle) -> Option<String> {
    let state = app.state::<DesktopRuntimeState>();
    state.pending_session.lock().await.take()
}

async fn shutdown_control_plane(app: AppHandle) {
    let state = app.state::<DesktopRuntimeState>();
    let mut supervisor = state.supervisor.lock().await;
    if let Some(handle) = supervisor.as_mut() {
        let mut child = handle.control_plane_child.lock().await;
        if let Some(process) = child.as_mut() {
            let _ = process.start_kill();
        }
    }
}

fn write_embedded_assets(root: &Path) -> io::Result<()> {
    for (relative_path, contents) in EMBEDDED_ASSETS {
        let destination = root.join(relative_path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(destination, contents)?;
    }
    Ok(())
}

fn next_available_port() -> io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

fn parse_activation_request<I, S>(args: I) -> ActivationRequest
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut activate_desktop = false;
    let mut session_id = None;
    let mut iter = args.into_iter().map(Into::into);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--activate-desktop" => activate_desktop = true,
            "--session" => session_id = iter.next(),
            _ => {}
        }
    }
    ActivationRequest {
        activate_desktop,
        session_id,
    }
}

#[cfg(test)]
mod tests {
    use super::{ActivationRequest, build_operator_url, parse_activation_request};

    #[test]
    fn parses_activation_request_without_session() {
        let activation = parse_activation_request(["--activate-desktop"]);
        assert_eq!(
            activation,
            ActivationRequest {
                activate_desktop: true,
                session_id: None,
            }
        );
    }

    #[test]
    fn parses_activation_request_with_session() {
        let activation = parse_activation_request(["--activate-desktop", "--session", "abc123"]);
        assert_eq!(
            activation,
            ActivationRequest {
                activate_desktop: true,
                session_id: Some("abc123".to_string()),
            }
        );
    }

    #[test]
    fn builds_operator_url_with_query() {
        assert_eq!(
            build_operator_url("http://127.0.0.1:3000", Some("session-1")),
            "http://127.0.0.1:3000/?session=session-1"
        );
    }
}
