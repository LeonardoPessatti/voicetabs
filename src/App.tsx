import { useEffect } from "react";
import { useTranslation } from "react-i18next";

import { CaptureToggle } from "./components/CaptureToggle";
import { SettingsDrawer } from "./components/SettingsDrawer";
import { SttStatusDot } from "./components/SttStatusDot";
import { TabBody } from "./components/TabBody";
import { TabStrip } from "./components/TabStrip";
import { useCaptureStore } from "./stores/captureStore";
import { useSegmentsStore } from "./stores/segmentsStore";
import { useSettingsStore } from "./stores/settingsStore";
import { useSttStore } from "./stores/sttStore";
import { useTabsStore } from "./stores/tabsStore";

export default function App() {
  const { t } = useTranslation();
  const tabs = useTabsStore();
  const settings = useSettingsStore();
  const capture = useCaptureStore();
  const stt = useSttStore();
  const segments = useSegmentsStore();

  useEffect(() => {
    void (async () => {
      await settings.load();
      await tabs.load();
      await capture.refresh();
      await capture.loadMode();
      await capture.loadBinding();
      capture.startPolling();
      await stt.refresh();
      stt.startPolling();
      await segments.startListening();
    })();
    return () => {
      capture.stopPolling();
      stt.stopPolling();
      segments.stopListening();
    };
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
      <TabBody activeTabId={tabs.activeTabId} />
      <footer className="app-footer">
        <CaptureToggle
          status={capture.status}
          onStart={() => void capture.startCapture()}
          onStop={() => void capture.stopCapture()}
        />
        <SttStatusDot status={stt.status} />
        <button onClick={settings.openDrawer} className="footer-button">
          ⚙ {t("settings.open")}
        </button>
      </footer>
      <SettingsDrawer />
    </main>
  );
}
