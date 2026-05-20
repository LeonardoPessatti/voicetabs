import { useEffect } from "react";
import { useTranslation } from "react-i18next";

import { TabStrip } from "./components/TabStrip";
import { useTabsStore } from "./stores/tabsStore";

export default function App() {
  const { t } = useTranslation();
  const {
    tabs,
    activeTabId,
    loaded,
    load,
    setActive,
    createTab,
    renameTab,
    deleteTab,
    reorderTabs,
  } = useTabsStore();

  useEffect(() => {
    if (!loaded) {
      void load();
    }
  }, [loaded, load]);

  if (!loaded) {
    return <main className="app" />;
  }

  return (
    <main className="app">
      <TabStrip
        tabs={tabs}
        activeId={activeTabId}
        onSelect={(id) => void setActive(id)}
        onCreate={() => void createTab(t("tabs.newTabTitle"))}
        onRename={(id, title) => void renameTab(id, title)}
        onClose={(id) => void deleteTab(id)}
        onReorder={(ordered) => void reorderTabs(ordered)}
      />
      <div className="tab-body">
        {tabs.length > 0 && activeTabId != null && (
          <p style={{ padding: 16, color: "#888" }}>
            {tabs.find((t) => t.id === activeTabId)?.title}
          </p>
        )}
      </div>
    </main>
  );
}
