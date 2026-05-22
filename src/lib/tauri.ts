import { invoke } from "@tauri-apps/api/core";

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
