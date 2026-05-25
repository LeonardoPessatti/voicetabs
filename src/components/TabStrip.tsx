import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  closestCenter,
  DndContext,
  DragEndEvent,
  PointerSensor,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import {
  arrayMove,
  horizontalListSortingStrategy,
  SortableContext,
  useSortable,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";

import { Tab } from "../lib/tauri";
import { useSegmentsStore } from "../stores/segmentsStore";

type Props = {
  tabs: Tab[];
  activeId: number | null;
  onSelect: (id: number) => void;
  onCreate: () => void;
  onRename: (id: number, title: string) => void;
  onClose: (id: number) => void;
  onReorder: (orderedIds: number[]) => void;
};

function SortableTab(props: {
  tab: Tab;
  isActive: boolean;
  isRenaming: boolean;
  draft: string;
  setDraft: (v: string) => void;
  commitRename: () => void;
  cancelRename: () => void;
  onSelect: () => void;
  beginRename: () => void;
  onClose: () => void;
  closeLabel: string;
  segmentCount: number;
}) {
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: props.tab.id });

  const style = {
    transform: CSS.Transform.toString(transform),
    transition,
    opacity: isDragging ? 0.6 : 1,
  };

  return (
    <div
      ref={setNodeRef}
      style={style}
      aria-label={props.tab.title}
      className={`tab${props.isActive ? " tab--active" : ""}`}
      onClick={props.onSelect}
      onDoubleClick={props.beginRename}
      {...attributes}
      {...listeners}
      role="tab"
      aria-selected={props.isActive}
      tabIndex={props.isActive ? 0 : -1}
    >
      {props.isRenaming ? (
        <input
          autoFocus
          value={props.draft}
          onChange={(e) => props.setDraft(e.target.value)}
          onBlur={props.commitRename}
          onKeyDown={(e) => {
            if (e.key === "Enter") props.commitRename();
            if (e.key === "Escape") props.cancelRename();
          }}
          onPointerDown={(e) => e.stopPropagation()}
          onMouseDown={(e) => e.stopPropagation()}
        />
      ) : (
        <span className="tab__title">{props.tab.title}</span>
      )}
      {props.segmentCount > 0 && (
        <span
          className="tab__count"
          aria-label={`${props.segmentCount} segments`}
        >
          {props.segmentCount}
        </span>
      )}
      <button
        className="tab__close"
        aria-label={props.closeLabel}
        onClick={(e) => {
          e.stopPropagation();
          props.onClose();
        }}
        onPointerDown={(e) => e.stopPropagation()}
      >
        ×
      </button>
    </div>
  );
}

export function TabStrip({
  tabs,
  activeId,
  onSelect,
  onCreate,
  onRename,
  onClose,
  onReorder,
}: Props) {
  const { t } = useTranslation();
  const [renamingId, setRenamingId] = useState<number | null>(null);
  const [draft, setDraft] = useState("");
  const segmentsByTab = useSegmentsStore((s) => s.segmentsByTab);

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
  );

  function commitRename(id: number) {
    if (draft.trim().length > 0) onRename(id, draft.trim());
    setRenamingId(null);
  }

  function handleDragEnd(e: DragEndEvent) {
    const { active, over } = e;
    if (!over || active.id === over.id) return;
    const oldIndex = tabs.findIndex((t) => t.id === Number(active.id));
    const newIndex = tabs.findIndex((t) => t.id === Number(over.id));
    if (oldIndex < 0 || newIndex < 0) return;
    const reordered = arrayMove(tabs, oldIndex, newIndex);
    onReorder(reordered.map((t) => t.id));
  }

  return (
    <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={handleDragEnd}>
      <div className="tab-strip" role="tablist">
        <SortableContext items={tabs.map((t) => t.id)} strategy={horizontalListSortingStrategy}>
          {tabs.map((tab) => (
            <SortableTab
              key={tab.id}
              tab={tab}
              isActive={tab.id === activeId}
              isRenaming={renamingId === tab.id}
              draft={draft}
              setDraft={setDraft}
              commitRename={() => commitRename(tab.id)}
              cancelRename={() => setRenamingId(null)}
              onSelect={() => onSelect(tab.id)}
              beginRename={() => {
                setRenamingId(tab.id);
                setDraft(tab.title);
              }}
              onClose={() => onClose(tab.id)}
              closeLabel={t("tabs.close")}
              segmentCount={(segmentsByTab[tab.id] ?? []).length}
            />
          ))}
        </SortableContext>
        <button
          className="tab-strip__new"
          aria-label={t("tabs.newTab")}
          onClick={onCreate}
        >
          +
        </button>
      </div>
    </DndContext>
  );
}
