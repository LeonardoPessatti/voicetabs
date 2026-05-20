import { create } from "zustand";

import i18n from "../i18n";
import { settingsApi, tabsApi, Tab } from "../lib/tauri";

const ACTIVE_KEY = "active_tab_id";

function defaultTitle(): string {
  return i18n.t("tabs.newTabTitle");
}

type TabsState = {
  tabs: Tab[];
  activeTabId: number | null;
  loaded: boolean;

  load: () => Promise<void>;
  setActive: (id: number) => Promise<void>;
  createTab: (title: string) => Promise<Tab>;
  renameTab: (id: number, title: string) => Promise<void>;
  deleteTab: (id: number) => Promise<void>;
  reorderTabs: (orderedIds: number[]) => Promise<void>;
};

export const useTabsStore = create<TabsState>((set, get) => ({
  tabs: [],
  activeTabId: null,
  loaded: false,

  async load() {
    const tabs = await tabsApi.list();
    const activeRaw = await settingsApi.get(ACTIVE_KEY);
    let activeTabId: number | null = activeRaw === null ? null : Number(activeRaw);

    if (tabs.length === 0) {
      const fresh = await tabsApi.create(defaultTitle());
      set({ tabs: [fresh], activeTabId: fresh.id, loaded: true });
      await settingsApi.set(ACTIVE_KEY, String(fresh.id));
      return;
    }

    if (activeTabId === null || !tabs.find((t) => t.id === activeTabId)) {
      activeTabId = tabs[0].id;
      await settingsApi.set(ACTIVE_KEY, String(activeTabId));
    }

    set({ tabs, activeTabId, loaded: true });
  },

  async setActive(id) {
    set({ activeTabId: id });
    await settingsApi.set(ACTIVE_KEY, String(id));
  },

  async createTab(title) {
    const tab = await tabsApi.create(title);
    set((s) => ({ tabs: [...s.tabs, tab], activeTabId: tab.id }));
    await settingsApi.set(ACTIVE_KEY, String(tab.id));
    return tab;
  },

  async renameTab(id, title) {
    await tabsApi.rename(id, title);
    set((s) => ({
      tabs: s.tabs.map((t) => (t.id === id ? { ...t, title } : t)),
    }));
  },

  async deleteTab(id) {
    await tabsApi.delete(id);
    const remaining = get().tabs.filter((t) => t.id !== id);

    if (remaining.length === 0) {
      const fresh = await tabsApi.create(defaultTitle());
      set({ tabs: [fresh], activeTabId: fresh.id });
      await settingsApi.set(ACTIVE_KEY, String(fresh.id));
      return;
    }

    let nextActive = get().activeTabId;
    if (nextActive === id) {
      nextActive = remaining[0].id;
      await settingsApi.set(ACTIVE_KEY, String(nextActive));
    }
    set({ tabs: remaining, activeTabId: nextActive });
  },

  async reorderTabs(orderedIds) {
    await tabsApi.reorder(orderedIds);
    const byId = new Map(get().tabs.map((t) => [t.id, t]));
    const reordered = orderedIds
      .map((id, idx) => {
        const t = byId.get(id);
        return t ? { ...t, order_idx: idx } : null;
      })
      .filter((t): t is Tab => t !== null);
    set({ tabs: reordered });
  },
}));
