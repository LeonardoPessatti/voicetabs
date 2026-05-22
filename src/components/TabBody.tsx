import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { SegmentCard } from "./SegmentCard";
import { RetranscribeMode, Segment, segmentsApi } from "../lib/tauri";
import { useSegmentsStore } from "../stores/segmentsStore";

type Props = {
  activeTabId: number | null;
};

export function TabBody({ activeTabId }: Props) {
  const { t } = useTranslation();
  const segmentsByTab = useSegmentsStore((s) => s.segmentsByTab);
  const loadForTab = useSegmentsStore((s) => s.loadForTab);
  const updateSegmentText = useSegmentsStore((s) => s.updateSegmentText);
  const deleteSegment = useSegmentsStore((s) => s.deleteSegment);
  const applyRetranscribe = useSegmentsStore((s) => s.applyRetranscribe);

  const [transcribingId, setTranscribingId] = useState<number | null>(null);

  const segments: Segment[] = activeTabId == null
    ? []
    : segmentsByTab[activeTabId] ?? [];

  // Load segments whenever the active tab changes.
  useEffect(() => {
    if (activeTabId != null) {
      void loadForTab(activeTabId);
    }
  }, [activeTabId, loadForTab]);

  // Auto-scroll the list to the bottom whenever a new segment lands.
  const bottomRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [segments.length]);

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

  if (activeTabId == null) {
    return null;
  }

  if (segments.length === 0) {
    return (
      <div className="tab-body tab-body--empty">
        <p>{t("segments.emptyState")}</p>
      </div>
    );
  }

  return (
    <div className="tab-body">
      <div className="tab-body__list">
        {segments.map((segment) => (
          <SegmentCard
            key={segment.id}
            segment={segment}
            onEdit={(id, text) => void updateSegmentText(id, text)}
            onDelete={(id) => void deleteSegment(id)}
            onRetranscribe={handleRetranscribe}
            isTranscribing={transcribingId === segment.id}
          />
        ))}
        <div ref={bottomRef} />
      </div>
    </div>
  );
}
