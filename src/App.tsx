import { useEffect } from "react";
import { useTranslation } from "react-i18next";

import { TabStrip } from "./components/TabStrip";
import { SettingsDrawer } from "./components/SettingsDrawer";
import { useTabsStore } from "./stores/tabsStore";
import { useSettingsStore } from "./stores/settingsStore";

export default function App() {
  const { t } = useTranslation();
  const tabs = useTabsStore();
  const settings = useSettingsStore();

  useEffect(() => {
    void (async () => {
      await settings.load();
      await tabs.load();
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    document.title = t("app.title");
  }, [t]);

  if (!tabs.loaded) {
    return <main className="app" />;
  }

  return (
    <main className="app">
      <TabStrip
        tabs={tabs.tabs}
        activeId={tabs.activeTabId}
        onSelect={(id) => void tabs.setActive(id)}
        onCreate={() => void tabs.createTab(t("tabs.newTabTitle"))}
        onRename={(id, title) => void tabs.renameTab(id, title)}
        onClose={(id) => void tabs.deleteTab(id)}
        onReorder={(ordered) => void tabs.reorderTabs(ordered)}
      />
      <div className="tab-body">
        {tabs.tabs.length > 0 && tabs.activeTabId != null && (
          <p style={{ padding: 16, color: "#888" }}>
            {tabs.tabs.find((t) => t.id === tabs.activeTabId)?.title}
          </p>
        )}
      </div>
      <footer className="app-footer">
        <span />
        <button onClick={settings.openDrawer} className="footer-button">
          ⚙ {t("settings.open")}
        </button>
      </footer>
      <SettingsDrawer />
    </main>
  );
}
