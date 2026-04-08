#![allow(clippy::result_large_err)]
use std::path::{Path, PathBuf};
use std::process::Stdio;

use base64::Engine;
use chrono::Utc;
use desktop_core::{
    ActionReceipt, ActionRequest, ArtifactRef, CursorPosition, MouseButton, Observation,
    ScreenshotData, StructuredError, WindowMetadata,
};
use serde_json::{Value, json};
use tokio::fs;
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct BackendOptions {
    pub display: String,
    pub artifacts_dir: PathBuf,
    pub browser_command: String,
}

#[derive(Debug, Clone)]
pub struct LinuxBackend {
    options: BackendOptions,
}

impl LinuxBackend {
    pub fn new(options: BackendOptions) -> Self {
        Self { options }
    }

    pub fn display(&self) -> &str {
        &self.options.display
    }

    pub fn artifacts_dir(&self) -> &Path {
        &self.options.artifacts_dir
    }

    pub fn browser_command(&self) -> &str {
        &self.options.browser_command
    }

    pub fn capabilities(&self) -> Vec<String> {
        let mut caps = vec![
            "screenshot".to_string(),
            "shell".to_string(),
            "filesystem".to_string(),
        ];
        if Self::tool_exists("xdotool") {
            caps.extend([
                "mouse".to_string(),
                "keyboard".to_string(),
                "window_focus".to_string(),
                "window_resize".to_string(),
            ]);
        }
        if Self::tool_exists("xprop") {
            caps.push("window_metadata".to_string());
        }
        if Self::tool_exists(&self.options.browser_command) {
            caps.push("browser_open".to_string());
        }
        caps
    }

    pub async fn observation(&self) -> Result<Observation, StructuredError> {
        let screenshot = self.capture_screenshot().await?;
        let active_window = self.active_window().await.ok();
        let cursor_position = self.cursor_position().await.ok();
        let active_window_title = active_window
            .as_ref()
            .and_then(|window| window.title.clone());
        Ok(Observation {
            captured_at: Utc::now(),
            capability_flags: self.capabilities(),
            active_window,
            cursor_position,
            browser: None,
            raw: json!({
                "display": self.options.display,
            }),
            summary: json!({
                "display": self.options.display,
                "active_window": active_window_title,
            }),
            screenshot,
        })
    }

    pub async fn screenshot_png(&self) -> Result<(Vec<u8>, PathBuf), StructuredError> {
        let screenshot = self.capture_screenshot().await?;
        let path = screenshot
            .artifact_path
            .clone()
            .ok_or_else(|| self.io_error("screenshot artifact path missing".to_string()))?;
        let bytes = fs::read(&path)
            .await
            .map_err(|error| self.io_error(error.to_string()))?;
        Ok((bytes, PathBuf::from(path)))
    }

    pub async fn perform_action(&self, action: ActionRequest) -> ActionReceipt {
        let started_at = Utc::now();
        let action_name = action.action_name().to_string();
        match self.perform_action_inner(action).await {
            Ok((result, artifacts)) => {
                ActionReceipt::success(&action_name, started_at, result, artifacts)
            }
            Err(error) => ActionReceipt::failure(&action_name, started_at, error),
        }
    }

    async fn perform_action_inner(
        &self,
        action: ActionRequest,
    ) -> Result<(Value, Vec<ArtifactRef>), StructuredError> {
        match action {
            ActionRequest::MouseMove { x, y, .. } => {
                self.run_xdotool(["mousemove", &x.to_string(), &y.to_string()])
                    .await?;
                Ok((json!({"x": x, "y": y}), vec![]))
            }
            ActionRequest::MouseClick { button, x, y, .. } => {
                if let (Some(x), Some(y)) = (x, y) {
                    self.run_xdotool(["mousemove", &x.to_string(), &y.to_string()])
                        .await?;
                }
                let button_number = match button.unwrap_or(MouseButton::Left) {
                    MouseButton::Left => "1",
                    MouseButton::Middle => "2",
                    MouseButton::Right => "3",
                };
                self.run_xdotool(["click", button_number]).await?;
                Ok((json!({"button": button_number}), vec![]))
            }
            ActionRequest::MouseDrag {
                start_x,
                start_y,
                end_x,
                end_y,
                ..
            } => {
                self.run_xdotool(["mousemove", &start_x.to_string(), &start_y.to_string()])
                    .await?;
                self.run_xdotool(["mousedown", "1"]).await?;
                self.run_xdotool(["mousemove", &end_x.to_string(), &end_y.to_string()])
                    .await?;
                self.run_xdotool(["mouseup", "1"]).await?;
                Ok((
                    json!({"start": [start_x, start_y], "end": [end_x, end_y]}),
                    vec![],
                ))
            }
            ActionRequest::KeyPress { key, .. } => {
                self.run_xdotool(["key", &key]).await?;
                Ok((json!({"key": key}), vec![]))
            }
            ActionRequest::TypeText { text, .. } => {
                self.run_xdotool(["type", "--delay", "1", &text]).await?;
                Ok((json!({"typed": text}), vec![]))
            }
            ActionRequest::Hotkey { keys, .. } => {
                let joined = keys.join("+");
                self.run_xdotool(["key", &joined]).await?;
                Ok((json!({"keys": keys}), vec![]))
            }
            ActionRequest::Scroll {
                delta_x: _,
                delta_y,
                ..
            } => {
                if delta_y == 0 {
                    return Err(self.unsupported(
                        "horizontal-only scroll is not supported by the xdotool fallback",
                    ));
                }
                let button = if delta_y > 0 { "4" } else { "5" };
                let clicks = (delta_y.abs().max(1) / 120) + 1;
                for _ in 0..clicks {
                    self.run_xdotool(["click", button]).await?;
                }
                Ok((
                    json!({"delta_y": delta_y, "emulated_clicks": clicks}),
                    vec![],
                ))
            }
            ActionRequest::OpenApp { name, .. } => {
                self.run_shell_background(&name).await?;
                Ok((json!({"command": name}), vec![]))
            }
            ActionRequest::FocusWindow { window_id, .. } => {
                self.run_xdotool(["windowactivate", &window_id]).await?;
                Ok((json!({"window_id": window_id}), vec![]))
            }
            ActionRequest::ResizeWindow {
                window_id, bounds, ..
            } => {
                self.run_xdotool([
                    "windowsize",
                    &window_id,
                    &bounds.width.to_string(),
                    &bounds.height.to_string(),
                ])
                .await?;
                self.run_xdotool([
                    "windowmove",
                    &window_id,
                    &bounds.x.to_string(),
                    &bounds.y.to_string(),
                ])
                .await?;
                Ok((json!({"window_id": window_id, "bounds": bounds}), vec![]))
            }
            ActionRequest::RunCommand {
                command, cwd, env, ..
            } => {
                let mut cmd = Command::new("sh");
                cmd.arg("-lc").arg(&command);
                cmd.env("DISPLAY", &self.options.display);
                if let Some(cwd) = cwd.as_ref() {
                    cmd.current_dir(cwd);
                }
                if let Some(env_map) = env {
                    for (key, value) in env_map {
                        cmd.env(key, value);
                    }
                }
                let output = cmd
                    .output()
                    .await
                    .map_err(|error| self.io_error(error.to_string()))?;
                Ok((
                    json!({
                        "stdout": String::from_utf8_lossy(&output.stdout),
                        "stderr": String::from_utf8_lossy(&output.stderr),
                        "exit_code": output.status.code(),
                    }),
                    vec![],
                ))
            }
            ActionRequest::ReadFile { path, .. } => {
                let contents = fs::read_to_string(&path)
                    .await
                    .map_err(|error| self.io_error(error.to_string()))?;
                Ok((json!({"path": path, "contents": contents}), vec![]))
            }
            ActionRequest::WriteFile { path, contents, .. } => {
                if let Some(parent) = Path::new(&path).parent() {
                    fs::create_dir_all(parent)
                        .await
                        .map_err(|error| self.io_error(error.to_string()))?;
                }
                fs::write(&path, contents.as_bytes())
                    .await
                    .map_err(|error| self.io_error(error.to_string()))?;
                Ok((
                    json!({"path": path, "bytes_written": contents.len()}),
                    vec![],
                ))
            }
            ActionRequest::BrowserOpen { url, .. } => {
                let escaped = url.replace('"', "\\\"").replace('\'', "'\\''");
                self.run_shell_background(&format!(
                    "{} '{}'",
                    self.options.browser_command, escaped
                ))
                .await?;
                Ok((json!({"url": url, "mode": "desktop_fallback"}), vec![]))
            }
            ActionRequest::BrowserGetDom { .. }
            | ActionRequest::BrowserClick { .. }
            | ActionRequest::BrowserType { .. }
            | ActionRequest::BrowserScreenshot { .. } => Err(self.unsupported(
                "browser-specialized actions are handled by the control-plane browser adapter",
            )),
        }
    }

    async fn capture_screenshot(&self) -> Result<ScreenshotData, StructuredError> {
        self.ensure_tool("import")?;
        fs::create_dir_all(&self.options.artifacts_dir)
            .await
            .map_err(|error| self.io_error(error.to_string()))?;
        let screenshot_path = self
            .options
            .artifacts_dir
            .join(format!("screenshot-{}.png", Utc::now().timestamp_millis()));
        let output = Command::new("import")
            .args([
                "-window",
                "root",
                screenshot_path.to_string_lossy().as_ref(),
            ])
            .env("DISPLAY", &self.options.display)
            .output()
            .await
            .map_err(|error| self.io_error(error.to_string()))?;
        if !output.status.success() {
            return Err(self.command_error(
                "import",
                String::from_utf8_lossy(&output.stderr).into_owned(),
            ));
        }
        let data = fs::read(&screenshot_path)
            .await
            .map_err(|error| self.io_error(error.to_string()))?;
        Ok(ScreenshotData {
            mime_type: "image/png".to_string(),
            data_base64: Some(base64::engine::general_purpose::STANDARD.encode(data)),
            width: None,
            height: None,
            artifact_path: Some(screenshot_path.to_string_lossy().to_string()),
        })
    }

    async fn active_window(&self) -> Result<WindowMetadata, StructuredError> {
        self.ensure_tool("xdotool")?;
        let id = self
            .run_command_capture("xdotool", ["getactivewindow"])
            .await?;
        let title = self
            .run_command_capture("xdotool", ["getactivewindow", "getwindowname"])
            .await
            .unwrap_or_default();
        let class_name = if Self::tool_exists("xprop") {
            self.run_command_capture("xprop", ["-id", id.trim(), "WM_CLASS"])
                .await
                .ok()
        } else {
            None
        };
        Ok(WindowMetadata {
            id: Some(id.trim().to_string()),
            title: Some(title.trim().to_string()).filter(|value| !value.is_empty()),
            class_name: class_name.map(|value| value.trim().to_string()),
        })
    }

    async fn cursor_position(&self) -> Result<CursorPosition, StructuredError> {
        self.ensure_tool("xdotool")?;
        let output = self
            .run_command_capture("xdotool", ["getmouselocation", "--shell"])
            .await?;
        let mut x = 0;
        let mut y = 0;
        let mut screen = None;
        for line in output.lines() {
            if let Some(value) = line.strip_prefix("X=") {
                x = value.parse().unwrap_or_default();
            } else if let Some(value) = line.strip_prefix("Y=") {
                y = value.parse().unwrap_or_default();
            } else if let Some(value) = line.strip_prefix("SCREEN=") {
                screen = Some(value.to_string());
            }
        }
        Ok(CursorPosition { x, y, screen })
    }

    async fn run_xdotool<I, S>(&self, args: I) -> Result<(), StructuredError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.ensure_tool("xdotool")?;
        let rendered: Vec<String> = args
            .into_iter()
            .map(|value| value.as_ref().to_string())
            .collect();
        let output = Command::new("xdotool")
            .args(&rendered)
            .env("DISPLAY", &self.options.display)
            .output()
            .await
            .map_err(|error| self.io_error(error.to_string()))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(self.command_error(
                "xdotool",
                String::from_utf8_lossy(&output.stderr).into_owned(),
            ))
        }
    }

    async fn run_shell_background(&self, command: &str) -> Result<(), StructuredError> {
        Command::new("sh")
            .arg("-lc")
            .arg(format!("{} >/dev/null 2>&1 &", command))
            .env("DISPLAY", &self.options.display)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| self.io_error(error.to_string()))?;
        Ok(())
    }

    async fn run_command_capture<I, S>(
        &self,
        binary: &str,
        args: I,
    ) -> Result<String, StructuredError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let rendered: Vec<String> = args
            .into_iter()
            .map(|value| value.as_ref().to_string())
            .collect();
        let output = Command::new(binary)
            .args(&rendered)
            .env("DISPLAY", &self.options.display)
            .output()
            .await
            .map_err(|error| self.io_error(error.to_string()))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(self.command_error(binary, String::from_utf8_lossy(&output.stderr).into_owned()))
        }
    }

    fn ensure_tool(&self, tool: &str) -> Result<(), StructuredError> {
        if Self::tool_exists(tool) {
            Ok(())
        } else {
            Err(self.missing_tool(tool))
        }
    }

    pub fn tool_exists(tool: &str) -> bool {
        std::process::Command::new("sh")
            .arg("-lc")
            .arg(format!("command -v {} >/dev/null 2>&1", tool))
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    fn missing_tool(&self, tool: &str) -> StructuredError {
        StructuredError {
            code: "missing_tool".to_string(),
            message: format!("Required system tool `{tool}` is not available in the sandbox."),
            retryable: false,
            category: "environment".to_string(),
            details: json!({"tool": tool}),
            artifact_refs: vec![],
        }
    }

    fn command_error(&self, binary: &str, stderr: String) -> StructuredError {
        StructuredError {
            code: "command_failed".to_string(),
            message: format!("Command `{binary}` failed."),
            retryable: true,
            category: "execution".to_string(),
            details: json!({"binary": binary, "stderr": stderr}),
            artifact_refs: vec![],
        }
    }

    fn unsupported(&self, message: &str) -> StructuredError {
        StructuredError {
            code: "unsupported".to_string(),
            message: message.to_string(),
            retryable: false,
            category: "unsupported".to_string(),
            details: json!({}),
            artifact_refs: vec![],
        }
    }

    fn io_error(&self, message: String) -> StructuredError {
        StructuredError {
            code: "io_error".to_string(),
            message,
            retryable: false,
            category: "io".to_string(),
            details: json!({}),
            artifact_refs: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_are_non_empty() {
        let backend = LinuxBackend::new(BackendOptions {
            display: ":99".to_string(),
            artifacts_dir: PathBuf::from("artifacts/test"),
            browser_command: "firefox".to_string(),
        });
        assert!(backend.capabilities().contains(&"shell".to_string()));
    }
}
