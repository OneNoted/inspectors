use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use chrono::{DateTime, Utc};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActionRequest {
    MouseMove {
        x: i32,
        y: i32,
        #[serde(default)]
        task_id: Option<String>,
    },
    MouseClick {
        button: Option<MouseButton>,
        x: Option<i32>,
        y: Option<i32>,
        #[serde(default)]
        task_id: Option<String>,
    },
    MouseDrag {
        start_x: i32,
        start_y: i32,
        end_x: i32,
        end_y: i32,
        #[serde(default)]
        task_id: Option<String>,
    },
    KeyPress {
        key: String,
        #[serde(default)]
        task_id: Option<String>,
    },
    TypeText {
        text: String,
        #[serde(default)]
        task_id: Option<String>,
    },
    Hotkey {
        keys: Vec<String>,
        #[serde(default)]
        task_id: Option<String>,
    },
    Scroll {
        delta_x: i32,
        delta_y: i32,
        #[serde(default)]
        task_id: Option<String>,
    },
    OpenApp {
        name: String,
        #[serde(default)]
        task_id: Option<String>,
    },
    FocusWindow {
        window_id: String,
        #[serde(default)]
        task_id: Option<String>,
    },
    ResizeWindow {
        window_id: String,
        bounds: Bounds,
        #[serde(default)]
        task_id: Option<String>,
    },
    RunCommand {
        command: String,
        cwd: Option<String>,
        env: Option<BTreeMap<String, String>>,
        #[serde(default)]
        task_id: Option<String>,
    },
    ReadFile {
        path: String,
        #[serde(default)]
        task_id: Option<String>,
    },
    WriteFile {
        path: String,
        contents: String,
        #[serde(default)]
        task_id: Option<String>,
    },
    BrowserOpen {
        url: String,
        #[serde(default)]
        task_id: Option<String>,
    },
    BrowserGetDom {
        #[serde(default)]
        task_id: Option<String>,
    },
    BrowserClick {
        selector: Option<String>,
        x: Option<i32>,
        y: Option<i32>,
        button: Option<MouseButton>,
        #[serde(default)]
        task_id: Option<String>,
    },
    BrowserType {
        selector: Option<String>,
        text: String,
        #[serde(default)]
        task_id: Option<String>,
    },
    BrowserScreenshot {
        #[serde(default)]
        task_id: Option<String>,
    },
}

impl ActionRequest {
    pub fn action_name(&self) -> &'static str {
        match self {
            Self::MouseMove { .. } => "mouse_move",
            Self::MouseClick { .. } => "mouse_click",
            Self::MouseDrag { .. } => "mouse_drag",
            Self::KeyPress { .. } => "key_press",
            Self::TypeText { .. } => "type_text",
            Self::Hotkey { .. } => "hotkey",
            Self::Scroll { .. } => "scroll",
            Self::OpenApp { .. } => "open_app",
            Self::FocusWindow { .. } => "focus_window",
            Self::ResizeWindow { .. } => "resize_window",
            Self::RunCommand { .. } => "run_command",
            Self::ReadFile { .. } => "read_file",
            Self::WriteFile { .. } => "write_file",
            Self::BrowserOpen { .. } => "browser_open",
            Self::BrowserGetDom { .. } => "browser_get_dom",
            Self::BrowserClick { .. } => "browser_click",
            Self::BrowserType { .. } => "browser_type",
            Self::BrowserScreenshot { .. } => "browser_screenshot",
        }
    }

    pub fn task_id(&self) -> Option<&str> {
        match self {
            Self::MouseMove { task_id, .. }
            | Self::MouseClick { task_id, .. }
            | Self::MouseDrag { task_id, .. }
            | Self::KeyPress { task_id, .. }
            | Self::TypeText { task_id, .. }
            | Self::Hotkey { task_id, .. }
            | Self::Scroll { task_id, .. }
            | Self::OpenApp { task_id, .. }
            | Self::FocusWindow { task_id, .. }
            | Self::ResizeWindow { task_id, .. }
            | Self::RunCommand { task_id, .. }
            | Self::ReadFile { task_id, .. }
            | Self::WriteFile { task_id, .. }
            | Self::BrowserOpen { task_id, .. }
            | Self::BrowserGetDom { task_id }
            | Self::BrowserClick { task_id, .. }
            | Self::BrowserType { task_id, .. }
            | Self::BrowserScreenshot { task_id } => task_id.as_deref(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ArtifactRef {
    pub kind: String,
    pub path: String,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StructuredError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub category: String,
    pub details: Value,
    #[serde(default)]
    pub artifact_refs: Vec<ArtifactRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ActionReceipt {
    pub status: String,
    pub receipt_id: String,
    pub action_type: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub result: Value,
    #[serde(default)]
    pub artifacts: Vec<ArtifactRef>,
    pub error: Option<StructuredError>,
}

impl ActionReceipt {
    pub fn success(
        action_type: &str,
        started_at: DateTime<Utc>,
        result: Value,
        artifacts: Vec<ArtifactRef>,
    ) -> Self {
        Self {
            status: "ok".to_string(),
            receipt_id: Uuid::new_v4().to_string(),
            action_type: action_type.to_string(),
            started_at,
            completed_at: Utc::now(),
            result,
            artifacts,
            error: None,
        }
    }

    pub fn failure(action_type: &str, started_at: DateTime<Utc>, error: StructuredError) -> Self {
        Self {
            status: "error".to_string(),
            receipt_id: Uuid::new_v4().to_string(),
            action_type: action_type.to_string(),
            started_at,
            completed_at: Utc::now(),
            result: json!({}),
            artifacts: error.artifact_refs.clone(),
            error: Some(error),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ScreenshotData {
    pub mime_type: String,
    #[serde(default)]
    pub data_base64: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub artifact_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WindowMetadata {
    pub id: Option<String>,
    pub title: Option<String>,
    pub class_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CursorPosition {
    pub x: i32,
    pub y: i32,
    pub screen: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BrowserSnapshot {
    pub current_url: Option<String>,
    pub title: Option<String>,
    pub dom_html: Option<String>,
    #[serde(default)]
    pub console_logs: Vec<String>,
    #[serde(default)]
    pub network_events: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Observation {
    pub captured_at: DateTime<Utc>,
    pub screenshot: ScreenshotData,
    pub active_window: Option<WindowMetadata>,
    pub cursor_position: Option<CursorPosition>,
    #[serde(default)]
    pub capability_flags: Vec<String>,
    pub browser: Option<BrowserSnapshot>,
    pub raw: Value,
    pub summary: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ActionDescriptor {
    pub name: String,
    pub description: String,
    pub category: String,
    pub requires_approval: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeCapabilities {
    pub actions: Vec<ActionDescriptor>,
    pub provider: String,
    pub browser_mode: String,
    pub vm_mode: String,
    #[serde(default)]
    pub enrichments: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskStatus {
    pub task_id: String,
    pub state: String,
    pub paused: bool,
    pub approval_required: bool,
    pub current_goal: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CreateSessionRequest {
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default = "default_width")]
    pub width: u32,
    #[serde(default = "default_height")]
    pub height: u32,
    pub display: Option<String>,
    pub browser_command: Option<String>,
    pub boot: Option<String>,
    pub container_image: Option<String>,
    pub disable_kvm: Option<bool>,
    pub qemu_profile: Option<String>,
    pub shared_host_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionRecord {
    pub id: String,
    pub provider: String,
    pub qemu_profile: Option<String>,
    pub display: Option<String>,
    pub width: u32,
    pub height: u32,
    pub state: String,
    pub created_at: DateTime<Utc>,
    pub artifacts_dir: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub browser_command: Option<String>,
    pub runtime_base_url: Option<String>,
    pub viewer_url: Option<String>,
    pub bridge_status: Option<String>,
    pub readiness_state: Option<String>,
    pub bridge_error: Option<StructuredError>,
}

fn default_provider() -> String {
    "xvfb".to_string()
}
fn default_width() -> u32 {
    1440
}
fn default_height() -> u32 {
    900
}

pub fn default_available_actions() -> Vec<ActionDescriptor> {
    vec![
        (
            "mouse_move",
            "Move the cursor to absolute desktop coordinates",
            "desktop",
        ),
        (
            "mouse_click",
            "Click a mouse button, optionally after moving to coordinates",
            "desktop",
        ),
        (
            "mouse_drag",
            "Drag the mouse from one coordinate to another",
            "desktop",
        ),
        ("key_press", "Press a single key", "desktop"),
        (
            "type_text",
            "Type raw text into the focused input",
            "desktop",
        ),
        ("hotkey", "Press a combination of keys in order", "desktop"),
        ("scroll", "Scroll the active window or surface", "desktop"),
        (
            "open_app",
            "Launch an application command inside the sandbox session",
            "system",
        ),
        (
            "focus_window",
            "Attempt to focus a known X11 window id",
            "desktop",
        ),
        ("resize_window", "Resize and move an X11 window", "desktop"),
        (
            "run_command",
            "Run a shell command within the sandbox",
            "system",
        ),
        (
            "read_file",
            "Read a file from the sandbox filesystem",
            "filesystem",
        ),
        (
            "write_file",
            "Write a file in the sandbox filesystem",
            "filesystem",
        ),
        (
            "browser_open",
            "Open a URL with the active browser adapter",
            "browser",
        ),
        (
            "browser_get_dom",
            "Return the current DOM snapshot",
            "browser",
        ),
        (
            "browser_click",
            "Click using a selector or coordinates in browser mode",
            "browser",
        ),
        (
            "browser_type",
            "Type using a selector in browser mode",
            "browser",
        ),
        (
            "browser_screenshot",
            "Capture a browser-specific screenshot",
            "browser",
        ),
    ]
    .into_iter()
    .map(|(name, description, category)| ActionDescriptor {
        name: name.to_string(),
        description: description.to_string(),
        category: category.to_string(),
        requires_approval: false,
    })
    .collect()
}

pub fn capability_descriptor(provider: &str, enrichments: Vec<String>) -> RuntimeCapabilities {
    RuntimeCapabilities {
        actions: default_available_actions(),
        provider: provider.to_string(),
        browser_mode: "playwright".to_string(),
        vm_mode: if provider == "qemu" {
            "qemu".to_string()
        } else {
            "xvfb-dev".to_string()
        },
        enrichments,
    }
}

pub fn write_schema_bundle(out_dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(out_dir)?;
    let bundles = [
        (
            "action.schema.json",
            serde_json::to_vec_pretty(&schema_for!(ActionRequest))?,
        ),
        (
            "observation.schema.json",
            serde_json::to_vec_pretty(&schema_for!(Observation))?,
        ),
        (
            "error.schema.json",
            serde_json::to_vec_pretty(&schema_for!(StructuredError))?,
        ),
        (
            "task.schema.json",
            serde_json::to_vec_pretty(&schema_for!(TaskStatus))?,
        ),
    ];
    for (name, bytes) in bundles {
        fs::write(out_dir.join(name), bytes)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_names_are_stable() {
        let action = ActionRequest::MouseMove {
            x: 1,
            y: 2,
            task_id: None,
        };
        assert_eq!(action.action_name(), "mouse_move");
    }

    #[test]
    fn schema_bundle_writes() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_schema_bundle(temp.path()).expect("write schemas");
        assert!(temp.path().join("action.schema.json").exists());
    }
}
