import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { SegmentCard } from "./SegmentCard";
import { EmptyTabHint } from "./EmptyTabHint";
import { RetranscribeMode, Segment, segmentsApi } from "../lib/tauri";
import { useSegmentsStore } from "../stores/segmentsStore";
import { useSettingsStore } from "../stores/settingsStore";
import { useNearBottom } from "../hooks/useNearBottom";
import { writeText } from "../lib/clipboard";

type Props = {
  activeTabId: number | null;
};

export function TabBody({ activeTabId }: Props) {
  const { t } = useTranslation();
  const segmentsByTab = useSegmentsStore((s) => s.segmentsByTab);
  const loadingByTab = useSegmentsStore((s) => s.loading);
  const loadForTab = useSegmentsStore((s) => s.loadForTab);
  const updateSegmentText = useSegmentsStore((s) => s.updateSegmentText);
  const deleteSegment = useSegmentsStore((s) => s.deleteSegment);
  const applyRetranscribe = useSegmentsStore((s) => s.applyRetranscribe);
  const showTimestamps = useSettingsStore((s) => s.showTimestamps);

  const [transcribingId, setTranscribingId] = useState<number | null>(null);
  const [pendingNew, setPendingNew] = useState(false);

  const segments: Segment[] =
    activeTabId == null ? [] : segmentsByTab[activeTabId] ?? [];
  const loading = activeTabId == null ? false : !!loadingByTab[activeTabId];

  useEffect(() => {
    if (activeTabId != null) {
      void loadForTab(activeTabId);
    }
  }, [activeTabId, loadForTab]);

  const [setListRef, isNearBottom] = useNearBottom(80);
  const listRef = useRef<HTMLDivElement | null>(null);
  const prevCountRef = useRef<number>(segments.length);

  // Effect: when count grows, either auto-scroll or surface the pill.
  useEffect(() => {
    const prev = prevCountRef.current;
    prevCountRef.current = segments.length;
    if (segments.length <= prev) return; // only when a new segment lands
    if (isNearBottom && listRef.current) {
      listRef.current.scrollTo({
        top: listRef.current.scrollHeight,
        behavior: "smooth",
      });
    } else {
      setPendingNew(true);
    }
  }, [segments.length, isNearBottom]);

  // Dismiss the pill once the user scrolls back into the near-bottom zone.
  useEffect(() => {
    if (isNearBottom) setPendingNew(false);
  }, [isNearBottom]);

  // Reset state when switching tabs.
  useEffect(() => {
    prevCountRef.current = segments.length;
    setPendingNew(false);
    // Snap to bottom on tab change so the user starts at the newest content.
    queueMicrotask(() => {
      if (listRef.current) {
        listRef.current.scrollTop = listRef.current.scrollHeight;
      }
    });
    // We intentionally only reset on tab change, not on segments change.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeTabId]);

  async function handleRetranscribe(id: number, mode: RetranscribeMode) {
    setTranscribingId(id);
    try {
      const result = await segmentsApi.retranscribe(id, mode);
      applyRetranscribe(id, result);
    } catch (e) {
      console.error("retranscribe failed", e);
    } finally {
      setTranscribingId(null);
    }
  }

  async function handleCopyTab() {
    const joined = segments.map((s) => s.text).join("\n\n");
    await writeText(joined);
  }

  function scrollToBottom() {
    if (listRef.current) {
      listRef.current.scrollTo({
        top: listRef.current.scrollHeight,
        behavior: "smooth",
      });
    }
    setPendingNew(false);
  }

  function attachListRef(el: HTMLDivElement | null) {
    listRef.current = el;
    setListRef(el);
  }

  if (activeTabId == null) {
    return null;
  }

  return (
    <div className="tab-body">
      <header className="tab-body__header">
        <button
          className="tab-body__copy-tab"
          onClick={() => void handleCopyTab()}
          disabled={segments.length === 0}
        >
          {t("segments.copyTab")}
        </button>
      </header>

      {segments.length === 0 && !loading ? (
        <EmptyTabHint />
      ) : (
        <div ref={attachListRef} className="tab-body__list">
          {segments.map((segment) => (
            <SegmentCard
              key={segment.id}
              segment={segment}
              onEdit={(id, text) => void updateSegmentText(id, text)}
              onDelete={(id) => void deleteSegment(id)}
              onRetranscribe={handleRetranscribe}
              isTranscribing={transcribingId === segment.id}
              showTimestamps={showTimestamps}
            />
          ))}
        </div>
      )}

      {pendingNew && (
        <button className="tab-body__pill" onClick={scrollToBottom}>
          {t("segments.newTranscriptionPill")}
        </button>
      )}
    </div>
  );
}
