use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use uuid::Uuid;

fn test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(prefix: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct GuestRuntimeHarness {
    child: Child,
    port: u16,
}

impl GuestRuntimeHarness {
    async fn start(fake_bin_dir: &Path, artifacts_dir: &Path) -> Self {
        let port = next_port();
        let binary = env!("CARGO_BIN_EXE_guest-runtime");
        let path_env = std::env::var("PATH").unwrap_or_default();
        let child = Command::new(binary)
            .args([
                "--port",
                &port.to_string(),
                "--artifacts-dir",
                artifacts_dir.to_string_lossy().as_ref(),
            ])
            .env("PATH", format!("{}:{path_env}", fake_bin_dir.display()))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .expect("spawn guest-runtime");
        wait_for_health(port).await;
        Self { child, port }
    }

    fn json_request(&self, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
        json_request(self.port, method, path, body)
    }

    async fn shutdown(&mut self) {
        let _ = self.child.start_kill();
        let _ = self.child.wait().await;
    }
}

impl Drop for GuestRuntimeHarness {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

#[tokio::test]
async fn qemu_sessions_report_viewer_only_until_the_bridge_is_ready() {
    let _guard = test_lock().lock().await;
    let fake_bin_dir = TestDir::new("guest-runtime-fake-bin");
    write_fake_docker(fake_bin_dir.path(), DockerMode::HealthyViewerOnly);
    let artifacts_dir = TestDir::new("guest-runtime-artifacts");
    let mut runtime = GuestRuntimeHarness::start(fake_bin_dir.path(), artifacts_dir.path()).await;

    let (create_status, create_payload) = runtime.json_request(
        "POST",
        "/api/sessions",
        Some(&json!({
            "provider": "qemu",
            "boot": "alpine",
            "width": 1280,
            "height": 720,
        })),
    );
    assert_eq!(
        create_status, 201,
        "unexpected create payload: {create_payload}"
    );
    let session = &create_payload["session"];
    assert_eq!(session["provider"], "qemu");
    assert_eq!(session["qemu_profile"], "product");
    assert_eq!(session["bridge_status"], "viewer_only");
    assert_eq!(session["readiness_state"], "booting");
    assert!(session["viewer_url"].as_str().is_some());
    assert_eq!(session["live_desktop_view"]["mode"], "stream");
    assert_eq!(
        session["live_desktop_view"]["provider_surface"],
        "qemu_novnc"
    );
    assert_eq!(
        session["live_desktop_view"]["matches_action_plane"],
        Value::Bool(true)
    );

    let session_id = session["id"].as_str().expect("session id");
    let (actions_status, actions_payload) =
        runtime.json_request("GET", &format!("/api/sessions/{session_id}/actions"), None);
    assert_eq!(
        actions_status, 200,
        "unexpected actions payload: {actions_payload}"
    );
    assert_eq!(actions_payload["provider"], "qemu");
    assert_eq!(actions_payload["browser_mode"], "viewer_only");
    assert_eq!(
        actions_payload["actions"].as_array().map(Vec::len),
        Some(0),
        "viewer-only QEMU sessions should not advertise runtime actions"
    );

    let (observation_status, observation_payload) = runtime.json_request(
        "GET",
        &format!("/api/sessions/{session_id}/observation"),
        None,
    );
    assert_eq!(
        observation_status, 409,
        "unexpected observation payload: {observation_payload}"
    );
    assert_eq!(
        observation_payload["error"]["code"],
        "provider_bridge_unavailable"
    );
    assert_eq!(
        observation_payload["error"]["details"]["bridge_status"],
        "viewer_only"
    );
    assert_eq!(
        observation_payload["error"]["details"]["readiness_state"],
        "booting"
    );
    assert_eq!(observation_payload["error"]["details"]["provider"], "qemu");

    let (delete_status, delete_payload) =
        runtime.json_request("DELETE", &format!("/api/sessions/{session_id}"), None);
    assert_eq!(
        delete_status, 200,
        "unexpected delete payload: {delete_payload}"
    );

    runtime.shutdown().await;
}

#[tokio::test]
async fn qemu_boot_failures_include_container_logs() {
    let _guard = test_lock().lock().await;
    let fake_bin_dir = TestDir::new("guest-runtime-fake-bin");
    write_fake_docker(fake_bin_dir.path(), DockerMode::BootFailure);
    let artifacts_dir = TestDir::new("guest-runtime-artifacts");
    let mut runtime = GuestRuntimeHarness::start(fake_bin_dir.path(), artifacts_dir.path()).await;

    let (status, payload) = runtime.json_request(
        "POST",
        "/api/sessions",
        Some(&json!({
            "provider": "qemu",
            "boot": "alpine",
        })),
    );
    assert_eq!(status, 424, "unexpected payload: {payload}");
    assert_eq!(payload["error"]["code"], "qemu_container_not_running");
    assert_eq!(
        payload["error"]["logs"].as_str().map(str::trim),
        Some("guest bootstrap failed: runtime health check timed out")
    );

    runtime.shutdown().await;
}

#[derive(Clone, Copy)]
enum DockerMode {
    HealthyViewerOnly,
    BootFailure,
}

fn write_fake_docker(dir: &Path, mode: DockerMode) {
    let script = match mode {
        DockerMode::HealthyViewerOnly => {
            r#"#!/bin/sh
set -eu
cmd="${1:-}"
shift || true
case "$cmd" in
  run)
    echo "stub-container-id"
    ;;
  inspect)
    if [ "${1:-}" = "-f" ] && [ "${2:-}" = "{{.State.Running}}" ]; then
      echo "true"
    elif [ "${1:-}" = "-f" ] && [ "${2:-}" = "{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}" ]; then
      echo "172.18.0.2"
    else
      echo "unexpected inspect args: $*" >&2
      exit 1
    fi
    ;;
  logs)
    echo "viewer boot completed without runtime bridge"
    ;;
  rm)
    echo "removed"
    ;;
  *)
    echo "unexpected docker command: $cmd $*" >&2
    exit 1
    ;;
esac
"#
        }
        DockerMode::BootFailure => {
            r#"#!/bin/sh
set -eu
cmd="${1:-}"
shift || true
case "$cmd" in
  run)
    echo "stub-container-id"
    ;;
  inspect)
    if [ "${1:-}" = "-f" ] && [ "${2:-}" = "{{.State.Running}}" ]; then
      echo "false"
    elif [ "${1:-}" = "-f" ] && [ "${2:-}" = "{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}" ]; then
      echo ""
    else
      echo "unexpected inspect args: $*" >&2
      exit 1
    fi
    ;;
  logs)
    echo "guest bootstrap failed: runtime health check timed out"
    ;;
  rm)
    echo "removed"
    ;;
  *)
    echo "unexpected docker command: $cmd $*" >&2
    exit 1
    ;;
esac
"#
        }
    };
    let docker_path = dir.join("docker");
    fs::write(&docker_path, script).expect("write fake docker");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&docker_path)
            .expect("docker metadata")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&docker_path, perms).expect("chmod fake docker");
    }
}

fn next_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local addr")
        .port()
}

async fn wait_for_health(port: u16) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok((status, payload)) =
            std::panic::catch_unwind(|| json_request(port, "GET", "/health", None))
        {
            if status == 200 && payload["status"] == "ok" {
                return;
            }
        }
        assert!(
            Instant::now() < deadline,
            "guest-runtime did not become healthy on port {port}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn json_request(port: u16, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
    let body_text = body.map(Value::to_string).unwrap_or_default();
    let script = r#"
import json
import sys
import urllib.error
import urllib.request

method, url, body = sys.argv[1], sys.argv[2], sys.argv[3]
data = body.encode("utf-8") if body else None
request = urllib.request.Request(
    url,
    method=method,
    data=data,
    headers={"Content-Type": "application/json"},
)
try:
    with urllib.request.urlopen(request) as response:
        payload = response.read().decode("utf-8")
        print(json.dumps({"status": response.getcode(), "body": json.loads(payload) if payload else {}}))
except urllib.error.HTTPError as error:
    payload = error.read().decode("utf-8")
    print(json.dumps({"status": error.code, "body": json.loads(payload) if payload else {}}))
"#;
    let output = StdCommand::new("python3")
        .args([
            "-c",
            script,
            method,
            &format!("http://127.0.0.1:{port}{path}"),
            &body_text,
        ])
        .output()
        .expect("run python http client");
    assert!(
        output.status.success(),
        "python http client failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value =
        serde_json::from_slice(&output.stdout).expect("parse python http client output");
    (
        envelope["status"].as_u64().expect("status") as u16,
        envelope["body"].clone(),
    )
}
