export interface JsonObject { [key: string]: unknown; }

export interface ActionReceipt {
  status: 'ok' | 'error';
  receipt_id: string;
  action_type: string;
  started_at: string;
  completed_at: string;
  result: unknown;
  artifacts: { kind: string; path: string; mime_type?: string }[];
  error?: unknown;
}

export type ActionRequest =
  | { kind: 'mouse_move'; x: number; y: number; taskId?: string }
  | { kind: 'mouse_click'; button?: 'left' | 'middle' | 'right'; x?: number; y?: number; taskId?: string }
  | { kind: 'mouse_drag'; start_x: number; start_y: number; end_x: number; end_y: number; taskId?: string }
  | { kind: 'key_press'; key: string; taskId?: string }
  | { kind: 'type_text'; text: string; taskId?: string }
  | { kind: 'hotkey'; keys: string[]; taskId?: string }
  | { kind: 'scroll'; delta_x: number; delta_y: number; taskId?: string }
  | { kind: 'open_app'; name: string; run_as_user?: string; taskId?: string }
  | { kind: 'focus_window'; window_id: string; taskId?: string }
  | { kind: 'resize_window'; window_id: string; bounds: { x: number; y: number; width: number; height: number }; taskId?: string }
  | { kind: 'run_command'; command: string; cwd?: string; env?: Record<string, string>; run_as_user?: string; taskId?: string }
  | { kind: 'read_file'; path: string; taskId?: string }
  | { kind: 'write_file'; path: string; contents: string; taskId?: string }
  | { kind: 'browser_open'; url: string; taskId?: string }
  | { kind: 'browser_get_dom'; taskId?: string }
  | { kind: 'browser_click'; selector?: string; x?: number; y?: number; button?: 'left' | 'middle' | 'right'; taskId?: string }
  | { kind: 'browser_type'; selector?: string; text: string; taskId?: string }
  | { kind: 'browser_screenshot'; taskId?: string };

export interface RuntimeCapabilities {
  actions: { name: string; description: string; category: string; requires_approval: boolean }[];
  provider: string;
  browser_mode: string;
  vm_mode: string;
  enrichments: string[];
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

export interface SessionRecord {
  id: string;
  provider: string;
  qemu_profile?: string | null;
  display?: string | null;
  width: number;
  height: number;
  state: string;
  created_at: string;
  artifacts_dir: string;
  capabilities: string[];
  browser_command?: string | null;
  desktop_user?: string | null;
  desktop_home?: string | null;
  desktop_runtime_dir?: string | null;
  runtime_base_url?: string | null;
  viewer_url?: string | null;
  live_desktop_view?: LiveDesktopView | null;
  bridge_status?: string | null;
  readiness_state?: string | null;
  bridge_error?: unknown;
}

export interface TaskRecord {
  id: string;
  sessionId: string;
  description: string;
  status: 'pending' | 'running' | 'paused' | 'completed' | 'terminated' | 'failed';
  createdAt: string;
  updatedAt: string;
  thoughtSummary?: string;
  requireApproval?: boolean;
  lastReceipt?: ActionReceipt;
}
