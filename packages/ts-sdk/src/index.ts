export type JsonValue = string | number | boolean | null | JsonObject | JsonValue[];
export interface JsonObject { [key: string]: JsonValue; }

export type SessionProvider = 'xvfb' | 'qemu';
export type BrowserMode = 'disabled' | 'playwright';
export type VmMode = 'xvfb-dev' | 'qemu' | 'unavailable';
export type TaskStatus = 'pending' | 'running' | 'paused' | 'completed' | 'terminated' | 'failed';
export type MouseButton = 'left' | 'middle' | 'right';

export interface ArtifactRef {
  kind: string;
  path: string;
  mime_type?: string | null;
}

export interface StructuredError {
  code: string;
  message: string;
  retryable: boolean;
  category: string;
  details: JsonObject;
  artifact_refs: ArtifactRef[];
}

export interface ActionReceipt {
  status: 'ok' | 'error';
  receipt_id: string;
  action_type: string;
  started_at: string;
  completed_at: string;
  result: JsonValue;
  artifacts: ArtifactRef[];
  error?: StructuredError | null;
}

export interface ScreenshotData {
  mime_type: string;
  data_base64?: string;
  width?: number | null;
  height?: number | null;
  artifact_path?: string | null;
}

export interface WindowMetadata {
  id?: string | null;
  title?: string | null;
  class_name?: string | null;
}

export interface BrowserSnapshot {
  current_url?: string | null;
  title?: string | null;
  dom_html?: string | null;
  console_logs: string[];
  network_events: string[];
}

export interface LiveDesktopView {
  mode: 'stream' | 'screenshot_poll' | 'unavailable';
  status: 'ready' | 'degraded' | 'stale' | 'unavailable';
  provider_surface: string;
  matches_action_plane: boolean;
  canonical_url?: string | null;
  debug_url?: string | null;
  reason?: string | null;
  refresh_interval_ms?: number | null;
}

export interface ObservationEnvelope {
  captured_at: string;
  screenshot: ScreenshotData;
  active_window?: WindowMetadata | null;
  cursor_position?: { x: number; y: number; screen?: string | null } | null;
  capability_flags: string[];
  browser?: BrowserSnapshot | null;
  raw: JsonObject;
  summary: JsonObject;
  screenshot_url?: string;
  action_history?: JsonValue[];
}

export interface RuntimeCapabilities {
  actions: { name: string; description: string; category: string; requires_approval: boolean }[];
  provider: SessionProvider | 'unavailable';
  browserMode: BrowserMode;
  vmMode: VmMode;
  enrichments: string[];
}

export interface TaskRecord {
  id: string;
  sessionId: string;
  description: string;
  status: TaskStatus;
  createdAt: string;
  updatedAt: string;
  thoughtSummary?: string;
  requireApproval?: boolean;
  lastReceipt?: ActionReceipt;
}

export type ActionRequest =
  | { kind: 'mouse_move'; x: number; y: number; taskId?: string }
  | { kind: 'mouse_click'; button?: MouseButton; x?: number; y?: number; taskId?: string }
  | { kind: 'mouse_drag'; start_x: number; start_y: number; end_x: number; end_y: number; taskId?: string }
  | { kind: 'key_press'; key: string; taskId?: string }
  | { kind: 'type_text'; text: string; taskId?: string }
  | { kind: 'hotkey'; keys: string[]; taskId?: string }
  | { kind: 'scroll'; delta_x: number; delta_y: number; taskId?: string }
  | { kind: 'open_app'; name: string; taskId?: string }
  | { kind: 'focus_window'; window_id: string; taskId?: string }
  | { kind: 'resize_window'; window_id: string; bounds: { x: number; y: number; width: number; height: number }; taskId?: string }
  | { kind: 'run_command'; command: string; cwd?: string; env?: Record<string, string>; taskId?: string }
  | { kind: 'read_file'; path: string; taskId?: string }
  | { kind: 'write_file'; path: string; contents: string; taskId?: string }
  | { kind: 'browser_open'; url: string; taskId?: string }
  | { kind: 'browser_get_dom'; taskId?: string }
  | { kind: 'browser_click'; selector?: string; x?: number; y?: number; button?: MouseButton; taskId?: string }
  | { kind: 'browser_type'; selector?: string; text: string; taskId?: string }
  | { kind: 'browser_screenshot'; taskId?: string };

export interface CreateSessionRequest {
  provider?: SessionProvider;
  width?: number;
  height?: number;
  display?: string;
  browser_command?: string;
  boot?: string;
  container_image?: string;
  disable_kvm?: boolean;
  qemu_profile?: 'product' | 'regression';
  shared_host_path?: string;
}

export interface SessionRecord {
  id: string;
  provider: SessionProvider;
  qemu_profile?: 'product' | 'regression' | null;
  display?: string | null;
  width: number;
  height: number;
  state: 'running' | 'stopped' | 'error';
  created_at: string;
  artifacts_dir: string;
  capabilities: string[];
  browser_command?: string | null;
  runtime_base_url?: string | null;
  viewer_url?: string | null;
  live_desktop_view?: LiveDesktopView | null;
  bridge_status?: string | null;
  readiness_state?: string | null;
  bridge_error?: StructuredError | null;
}

export interface ReclaimStorageResponse {
  mode: 'report' | 'apply';
  runtime_root: string;
  cache_root: string;
  exports_root: string;
  candidate_count: number;
  candidates: { path: string; tier: string; kind: string; reason: string }[];
  reclaimed: string[];
}

export class ComputerUseClient {
  constructor(private readonly baseUrl = 'http://127.0.0.1:3000') {}

  private async request<T>(path: string, init?: RequestInit): Promise<T> {
    const res = await fetch(`${this.baseUrl}${path}`, {
      headers: { 'content-type': 'application/json', ...(init?.headers ?? {}) },
      ...init
    });
    if (!res.ok) {
      throw new Error(`request failed ${res.status}: ${await res.text()}`);
    }
    return res.json() as Promise<T>;
  }

  listAdapters() {
    return this.request<{ adapters: JsonValue[] }>('/api/adapters');
  }

  reclaimStorage(mode: 'report' | 'apply' = 'report') {
    return this.request<ReclaimStorageResponse>('/api/storage/reclaim', { method: 'POST', body: JSON.stringify({ mode }) });
  }

  createSession(request: CreateSessionRequest = {}) {
    return this.request<{ session: SessionRecord }>('/api/sessions', { method: 'POST', body: JSON.stringify(request) });
  }

  getSession(sessionId: string) {
    return this.request<{ session: SessionRecord; browser_adapter: boolean }>(`/api/sessions/${sessionId}`);
  }

  deleteSession(sessionId: string) {
    return this.request<{ ok: boolean }>(`/api/sessions/${sessionId}`, { method: 'DELETE' });
  }

  getObservation(sessionId: string) {
    return this.request<ObservationEnvelope>(`/api/sessions/${sessionId}/observation`);
  }

  getAvailableActions(sessionId: string) {
    return this.request<RuntimeCapabilities & { browser_adapter?: string[] }>(`/api/sessions/${sessionId}/actions`);
  }

  performAction(sessionId: string, action: ActionRequest) {
    return this.request<ActionReceipt>(`/api/sessions/${sessionId}/actions`, { method: 'POST', body: JSON.stringify(action) });
  }

  createTask(sessionId: string, description: string, thoughtSummary?: string) {
    return this.request<{ task: TaskRecord }>('/api/tasks', { method: 'POST', body: JSON.stringify({ session_id: sessionId, description, thought_summary: thoughtSummary }) });
  }

  getTaskStatus(taskId: string) {
    return this.request<{ task: TaskRecord }>(`/api/tasks/${taskId}`);
  }

  pause(taskId: string) { return this.request<{ task: TaskRecord }>(`/api/tasks/${taskId}/pause`, { method: 'POST' }); }
  resume(taskId: string) { return this.request<{ task: TaskRecord }>(`/api/tasks/${taskId}/resume`, { method: 'POST' }); }
  reset(taskId: string) { return this.request<{ task: TaskRecord }>(`/api/tasks/${taskId}/reset`, { method: 'POST' }); }
  terminate(taskId: string) { return this.request<{ task: TaskRecord }>(`/api/tasks/${taskId}/terminate`, { method: 'POST' }); }
}
