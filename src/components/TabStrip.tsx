import { useState } from "react";
import { useTranslation } from "react-i18next";

import { Tab } from "../lib/tauri";

type Props = {
  tabs: Tab[];
  activeId: number | null;
  onSelect: (id: number) => void;
  onCreate: () => void;
  onRename: (id: number, title: string) => void;
  onClose: (id: number) => void;
  onReorder: (orderedIds: number[]) => void;
};

export function TabStrip({
  tabs,
  activeId,
  onSelect,
  onCreate,
  onRename,
  onClose,
}: Props) {
  const { t } = useTranslation();
  const [renamingId, setRenamingId] = useState<number | null>(null);
  const [draft, setDraft] = useState("");

  function commitRename(id: number) {
    if (draft.trim().length > 0) onRename(id, draft.trim());
    setRenamingId(null);
  }

  return (
    <div className="tab-strip" role="tablist">
      {tabs.map((tab) => {
        const isActive = tab.id === activeId;
        return (
          <div
            key={tab.id}
            role="tab"
            aria-selected={isActive}
            tabIndex={isActive ? 0 : -1}
            aria-label={tab.title}
            className={`tab${isActive ? " tab--active" : ""}`}
            onClick={() => onSelect(tab.id)}
            onDoubleClick={() => {
              setRenamingId(tab.id);
              setDraft(tab.title);
            }}
          >
            {renamingId === tab.id ? (
              <input
                autoFocus
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                onBlur={() => commitRename(tab.id)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") commitRename(tab.id);
                  if (e.key === "Escape") setRenamingId(null);
                }}
              />
            ) : (
              <span className="tab__title">{tab.title}</span>
            )}
            <button
              className="tab__close"
              aria-label={t("tabs.close")}
              onClick={(e) => {
                e.stopPropagation();
                onClose(tab.id);
              }}
            >
              ×
            </button>
          </div>
        );
      })}
      <button
        className="tab-strip__new"
        aria-label={t("tabs.newTab")}
        onClick={onCreate}
      >
        +
      </button>
    </div>
  );
}
