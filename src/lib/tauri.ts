import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";

export type Tab = {
  id: number;
  title: string;
  order_idx: number;
  created_at: number;
  updated_at: number;
};

export type CommandError = {
  code: string;
  message: string;
};

export const tabsApi = {
  list(): Promise<Tab[]> {
    return invoke<Tab[]>("tabs_list");
  },
  create(title: string): Promise<Tab> {
    return invoke<Tab>("tabs_create", { title });
  },
  rename(id: number, title: string): Promise<void> {
    return invoke<void>("tabs_rename", { id, title });
  },
  delete(id: number): Promise<void> {
    return invoke<void>("tabs_delete", { id });
  },
  reorder(orderedIds: number[]): Promise<void> {
    return invoke<void>("tabs_reorder", { orderedIds });
  },
  setActive(id: number): Promise<void> {
    return invoke<void>("tabs_set_active", { id });
  },
};

export const settingsApi = {
  get(key: string): Promise<string | null> {
    return invoke<string | null>("settings_get", { key });
  },
  async getJson<T>(key: string): Promise<T | null> {
    const raw = await this.get(key);
    return raw === null ? null : (JSON.parse(raw) as T);
  },
  set(key: string, value: string): Promise<void> {
    return invoke<void>("settings_set", { key, value });
  },
  setJson<T>(key: string, value: T): Promise<void> {
    return this.set(key, JSON.stringify(value));
  },
};

export type CaptureStatus =
  | { state: "idle" }
  | { state: "capturing"; device_name: string | null }
  | { state: "error"; message: string };

export const captureApi = {
  start(): Promise<void> {
    return invoke<void>("capture_start");
  },
  stop(): Promise<void> {
    return invoke<void>("capture_stop");
  },
  status(): Promise<CaptureStatus> {
    return invoke<CaptureStatus>("capture_status");
  },
};

export type SttStatus =
  | { state: "loading"; backend: string }
  | { state: "ready"; backend: string; model_id: string }
  | { state: "restarting"; backend: string }
  | { state: "error"; message: string };

export const sttApi = {
  status(): Promise<SttStatus> {
    return invoke<SttStatus>("stt_status");
  },
};

export type TranscriptionEvent = {
  request_id: string;
  text: string;
  avg_logprob: number;
  no_speech_prob: number;
  duration_ms: number;
  started_at_ms: number;
  ended_at_ms: number;
  audio_path: string;
};

export type Segment = {
  id: number;
  tab_id: number;
  position: number;
  text: string;
  original_text: string;
  audio_path: string;
  started_at: number;
  ended_at: number;
  duration_ms: number;
  vocab_snapshot: string;
  avg_logprob: number;
  no_speech_prob: number;
  model_id: string;
};

export type RetranscribeMode = "current" | "snapshot";

export type RetranscribeResult = {
  text: string;
  model_id: string;
  avg_logprob: number;
  no_speech_prob: number;
};

export const segmentsApi = {
  listForTab(tabId: number): Promise<Segment[]> {
    return invoke<Segment[]>("segments_list_for_tab", { tabId });
  },
  update(id: number, text: string): Promise<void> {
    return invoke<void>("segments_update", { id, text });
  },
  delete(id: number): Promise<void> {
    return invoke<void>("segments_delete", { id });
  },
  retranscribe(id: number, mode: RetranscribeMode): Promise<RetranscribeResult> {
    return invoke<RetranscribeResult>("segments_retranscribe", { id, mode });
  },
};

export function listenSegmentCreated(
  handler: (segment: Segment) => void,
): Promise<UnlistenFn> {
  return listen<Segment>("segment-created", (e) => handler(e.payload));
}

// --- Hotkey ---
export type BindingKind = "key" | "mouse";
export type Binding = { kind: BindingKind; code: string };

export const hotkeyApi = {
  async get(): Promise<Binding | null> {
    return await invoke<Binding | null>("hotkey_get_binding");
  },
  async set(binding: Binding): Promise<void> {
    await invoke("hotkey_set_binding", { binding });
  },
  async clear(): Promise<void> {
    await invoke("hotkey_clear_binding");
  },
  async captureNext(): Promise<Binding> {
    // Blocks on the backend for up to 15s; the UI shows "Pressione…" until
    // it resolves or throws.
    return await invoke<Binding>("hotkey_capture_next");
  },
};

// --- Capture mode ---
export type CaptureMode = "always_on" | "ptt";

export const captureModeApi = {
  async get(): Promise<CaptureMode> {
    return (await invoke<string>("capture_get_mode")) as CaptureMode;
  },
  async set(mode: CaptureMode): Promise<void> {
    await invoke("capture_set_mode", { mode });
  },
};

// --- Backend (STT) ---
export type BackendKind = "local" | "openai";

export const backendApi = {
  get(): Promise<BackendKind> {
    return invoke<BackendKind>("backend_get");
  },
  set(kind: BackendKind): Promise<void> {
    return invoke<void>("backend_set", { kind });
  },
  openaiKeyStatus(): Promise<boolean> {
    return invoke<boolean>("openai_key_status");
  },
  openaiKeySet(value: string): Promise<void> {
    return invoke<void>("openai_key_set", { value });
  },
  openaiKeyClear(): Promise<void> {
    return invoke<void>("openai_key_clear");
  },
};
