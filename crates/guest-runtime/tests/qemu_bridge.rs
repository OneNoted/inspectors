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

#[tokio::test]
async fn deleting_qemu_sessions_removes_runtime_artifacts() {
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
    let session_id = session["id"].as_str().expect("session id");
    let session_artifacts = PathBuf::from(
        session["artifacts_dir"]
            .as_str()
            .expect("session artifacts dir"),
    );
    fs::write(session_artifacts.join("data.img"), b"artifact").expect("seed artifact");
    assert!(session_artifacts.exists());

    let (delete_status, delete_payload) =
        runtime.json_request("DELETE", &format!("/api/sessions/{session_id}"), None);
    assert_eq!(
        delete_status, 200,
        "unexpected delete payload: {delete_payload}"
    );
    assert!(
        !session_artifacts.exists(),
        "session artifacts dir should be removed after delete"
    );

    runtime.shutdown().await;
}

#[tokio::test]
async fn startup_janitor_reaps_marked_runtime_directories() {
    let _guard = test_lock().lock().await;
    let fake_bin_dir = TestDir::new("guest-runtime-fake-bin");
    write_fake_docker(fake_bin_dir.path(), DockerMode::HealthyViewerOnly);
    let artifacts_dir = TestDir::new("guest-runtime-artifacts");
    let stale_dir = artifacts_dir.path().join(Uuid::new_v4().to_string());
    fs::create_dir_all(&stale_dir).expect("create stale dir");
    fs::write(stale_dir.join("data.img"), b"stale").expect("seed stale data");
    fs::write(
        stale_dir.join(".inspectors-storage.json"),
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "owner": "inspectors",
            "tier": "runtime",
            "kind": "session",
            "created_at": "2026-04-15T08:19:32Z",
            "session_id": stale_dir.file_name().and_then(|value| value.to_str()),
            "provider": "qemu",
            "qemu_profile": "product",
            "container_name": null,
            "process_id": null,
        }))
        .expect("serialize marker"),
    )
    .expect("write marker");

    let mut runtime = GuestRuntimeHarness::start(fake_bin_dir.path(), artifacts_dir.path()).await;
    assert!(
        !stale_dir.exists(),
        "startup janitor should remove stale marked runtime dirs"
    );

    runtime.shutdown().await;
}

#[tokio::test]
async fn startup_janitor_does_not_kill_running_prepare_containers() {
    let _guard = test_lock().lock().await;
    let fake_bin_dir = TestDir::new("guest-runtime-fake-bin");
    write_fake_docker(fake_bin_dir.path(), DockerMode::RunningPrepareContainer);
    let artifacts_dir = TestDir::new("guest-runtime-artifacts");

    let mut runtime = GuestRuntimeHarness::start(fake_bin_dir.path(), artifacts_dir.path()).await;
    let docker_log =
        fs::read_to_string(fake_bin_dir.path().join("docker.log")).expect("read fake docker log");

    assert!(
        !docker_log.contains("rm -f -v acu-image-prep-product-live"),
        "startup cleanup should not remove running prepare containers: {docker_log}"
    );

    runtime.shutdown().await;
}

#[tokio::test]
async fn reclaim_endpoint_reports_and_reclaims_legacy_runtime_state() {
    let _guard = test_lock().lock().await;
    let fake_bin_dir = TestDir::new("guest-runtime-fake-bin");
    write_fake_docker(fake_bin_dir.path(), DockerMode::HealthyViewerOnly);
    let artifacts_dir = TestDir::new("guest-runtime-artifacts");
    let legacy_dir = artifacts_dir.path().join(Uuid::new_v4().to_string());
    fs::create_dir_all(legacy_dir.join("seed")).expect("create legacy seed dir");
    fs::write(legacy_dir.join("data.img"), b"legacy").expect("write legacy data");
    let legacy_build_dir = artifacts_dir
        .path()
        .join("_qemu_images")
        .join("_build")
        .join("acu-qemu-product-legacy");
    fs::create_dir_all(&legacy_build_dir).expect("create legacy build dir");
    fs::write(legacy_build_dir.join("boot.qcow2"), b"legacy-build")
        .expect("write legacy build image");

    let mut runtime = GuestRuntimeHarness::start(fake_bin_dir.path(), artifacts_dir.path()).await;

    let (report_status, report_payload) = runtime.json_request(
        "POST",
        "/api/storage/reclaim",
        Some(&json!({ "mode": "report" })),
    );
    assert_eq!(
        report_status, 200,
        "unexpected report payload: {report_payload}"
    );
    assert_eq!(report_payload["candidate_count"], 2);
    let report_kinds = report_payload["candidates"]
        .as_array()
        .expect("report candidates")
        .iter()
        .map(|candidate| candidate["kind"].as_str().expect("candidate kind"))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        report_kinds,
        std::collections::BTreeSet::from(["legacy_runtime", "legacy_prepare_build_runtime_cache",])
    );
    assert!(legacy_dir.exists());
    assert!(legacy_build_dir.exists());

    let (apply_status, apply_payload) = runtime.json_request(
        "POST",
        "/api/storage/reclaim",
        Some(&json!({ "mode": "apply" })),
    );
    assert_eq!(
        apply_status, 200,
        "unexpected apply payload: {apply_payload}"
    );
    assert_eq!(apply_payload["candidate_count"], 2);
    let reclaimed_paths = apply_payload["reclaimed"]
        .as_array()
        .expect("reclaimed paths")
        .iter()
        .map(|path| path.as_str().expect("reclaimed path").to_string())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        reclaimed_paths,
        std::collections::BTreeSet::from([
            legacy_dir.to_string_lossy().to_string(),
            legacy_build_dir.to_string_lossy().to_string(),
        ])
    );
    assert!(
        !legacy_dir.exists(),
        "legacy runtime dir should be reclaimed"
    );
    assert!(
        !legacy_build_dir.exists(),
        "legacy build dir should be reclaimed"
    );

    runtime.shutdown().await;
}

#[tokio::test]
async fn reclaim_endpoint_does_not_reap_active_sessions() {
    let _guard = test_lock().lock().await;
    let fake_bin_dir = TestDir::new("guest-runtime-fake-bin");
    write_fake_docker(fake_bin_dir.path(), DockerMode::HealthyViewerOnly);
    let artifacts_dir = TestDir::new("guest-runtime-artifacts");
    let legacy_dir = artifacts_dir.path().join(Uuid::new_v4().to_string());
    fs::create_dir_all(legacy_dir.join("seed")).expect("create legacy seed dir");
    fs::write(legacy_dir.join("data.img"), b"legacy").expect("write legacy data");

    let mut runtime = GuestRuntimeHarness::start(fake_bin_dir.path(), artifacts_dir.path()).await;
    let (create_status, create_payload) = runtime.json_request(
        "POST",
        "/api/sessions",
        Some(&json!({
            "provider": "display",
            "display": ":0",
        })),
    );
    assert_eq!(
        create_status, 201,
        "unexpected create payload: {create_payload}"
    );
    let active_dir = PathBuf::from(
        create_payload["session"]["artifacts_dir"]
            .as_str()
            .expect("active artifacts dir"),
    );
    assert!(active_dir.exists(), "active session artifacts should exist");

    let (apply_status, apply_payload) = runtime.json_request(
        "POST",
        "/api/storage/reclaim",
        Some(&json!({ "mode": "apply" })),
    );
    assert_eq!(
        apply_status, 200,
        "unexpected apply payload: {apply_payload}"
    );
    assert_eq!(apply_payload["candidate_count"], 1);
    assert_eq!(
        apply_payload["reclaimed"][0],
        legacy_dir.to_string_lossy().to_string()
    );
    assert!(
        active_dir.exists(),
        "active session artifacts should not be reclaimed"
    );

    runtime.shutdown().await;
}

#[tokio::test]
async fn reclaim_endpoint_does_not_reap_live_prepare_build_dirs() {
    let _guard = test_lock().lock().await;
    let fake_bin_dir = TestDir::new("guest-runtime-fake-bin");
    write_fake_docker(fake_bin_dir.path(), DockerMode::HealthyViewerOnly);
    let workspace = TestDir::new("guest-runtime-workspace");
    let runtime_root = workspace.path().join("runtime");
    fs::create_dir_all(&runtime_root).expect("create runtime root");
    let live_build_dir = workspace
        .path()
        .join("cache")
        .join("qemu")
        .join("_build")
        .join("acu-qemu-product-live");
    fs::create_dir_all(&live_build_dir).expect("create live build dir");
    fs::write(live_build_dir.join("boot.qcow2"), b"live-build").expect("write build image");
    fs::write(
        live_build_dir.join(".inspectors-storage.json"),
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "owner": "inspectors",
            "tier": "runtime",
            "kind": "prepare_build",
            "created_at": "2026-04-15T08:19:32Z",
            "provider": "qemu",
            "qemu_profile": "product",
            "container_name": null,
            "process_id": std::process::id(),
        }))
        .expect("serialize marker"),
    )
    .expect("write marker");

    let mut runtime = GuestRuntimeHarness::start(fake_bin_dir.path(), &runtime_root).await;

    let (report_status, report_payload) = runtime.json_request(
        "POST",
        "/api/storage/reclaim",
        Some(&json!({ "mode": "report" })),
    );
    assert_eq!(
        report_status, 200,
        "unexpected report payload: {report_payload}"
    );
    assert_eq!(report_payload["candidate_count"], 0);
    assert!(
        live_build_dir.exists(),
        "live prepare build dir should remain"
    );

    runtime.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn reclaim_endpoint_only_reports_successful_removals() {
    use std::os::unix::fs::PermissionsExt;

    let _guard = test_lock().lock().await;
    let fake_bin_dir = TestDir::new("guest-runtime-fake-bin");
    write_fake_docker(fake_bin_dir.path(), DockerMode::HealthyViewerOnly);
    let workspace = TestDir::new("guest-runtime-workspace");
    let runtime_root = workspace.path().join("runtime");
    fs::create_dir_all(&runtime_root).expect("create runtime root");

    let mut runtime = GuestRuntimeHarness::start(fake_bin_dir.path(), &runtime_root).await;

    let legacy_dir = runtime_root.join(Uuid::new_v4().to_string());
    fs::create_dir_all(legacy_dir.join("seed")).expect("create legacy seed dir");
    fs::write(legacy_dir.join("data.img"), b"legacy").expect("write legacy data");

    let original_permissions = fs::metadata(&runtime_root)
        .expect("runtime root metadata")
        .permissions();
    let mut readonly_permissions = original_permissions.clone();
    readonly_permissions.set_mode(0o555);
    fs::set_permissions(&runtime_root, readonly_permissions).expect("make runtime root readonly");

    let (apply_status, apply_payload) = runtime.json_request(
        "POST",
        "/api/storage/reclaim",
        Some(&json!({ "mode": "apply" })),
    );

    fs::set_permissions(&runtime_root, original_permissions).expect("restore runtime root perms");

    assert_eq!(
        apply_status, 200,
        "unexpected apply payload: {apply_payload}"
    );
    assert_eq!(apply_payload["candidate_count"], 1);
    assert_eq!(
        apply_payload["reclaimed"].as_array().map(Vec::len),
        Some(0),
        "failed deletions should not be reported as reclaimed"
    );
    assert!(
        legacy_dir.exists(),
        "legacy dir should remain after failed removal"
    );

    runtime.shutdown().await;
}

#[derive(Clone, Copy)]
enum DockerMode {
    HealthyViewerOnly,
    BootFailure,
    RunningPrepareContainer,
}

fn write_fake_docker(dir: &Path, mode: DockerMode) {
    let script = match mode {
        DockerMode::HealthyViewerOnly => {
            r#"#!/bin/sh
set -eu
cmd="${1:-}"
shift || true
case "$cmd" in
  ps)
    exit 0
    ;;
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
  ps)
    exit 0
    ;;
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
        DockerMode::RunningPrepareContainer => {
            r#"#!/bin/sh
set -eu
cmd="${1:-}"
shift || true
echo "$cmd $*" >> "$(dirname "$0")/docker.log"
case "$cmd" in
  ps)
    args=" $* "
    if printf "%s" "$args" | grep -q "name=acu-image-prep-" && printf "%s" "$args" | grep -q "status=exited"; then
      exit 0
    fi
    if printf "%s" "$args" | grep -q "name=acu-image-prep-"; then
      echo "acu-image-prep-product-live"
      exit 0
    fi
    exit 0
    ;;
  run)
    echo "stub-container-id"
    ;;
  inspect)
    if [ "${1:-}" = "-f" ] && [ "${2:-}" = "{{.State.Running}}" ]; then
      echo "true"
    elif [ "${1:-}" = "-f" ] && [ "${2:-}" = "{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}" ]; then
      echo "172.18.0.2"
    else
      echo ""
    fi
    ;;
  logs)
    echo "ok"
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
