import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { TabStrip } from "../components/TabStrip";
import { Tab } from "../lib/tauri";

const tab = (id: number, title: string, order_idx: number): Tab => ({
  id,
  title,
  order_idx,
  created_at: 0,
  updated_at: 0,
});

describe("TabStrip", () => {
  it("renders one button per tab", () => {
    render(
      <TabStrip
        tabs={[tab(1, "Alpha", 0), tab(2, "Beta", 1)]}
        activeId={1}
        onSelect={() => {}}
        onCreate={() => {}}
        onRename={() => {}}
        onClose={() => {}}
        onReorder={() => {}}
      />,
    );
    expect(screen.getByRole("tab", { name: /Alpha/ })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: /Beta/ })).toBeInTheDocument();
  });

  it("marks the active tab with aria-selected", () => {
    render(
      <TabStrip
        tabs={[tab(1, "Alpha", 0), tab(2, "Beta", 1)]}
        activeId={2}
        onSelect={() => {}}
        onCreate={() => {}}
        onRename={() => {}}
        onClose={() => {}}
        onReorder={() => {}}
      />,
    );
    expect(screen.getByRole("tab", { name: /Beta/ })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("fires onSelect when clicking a tab", () => {
    const onSelect = vi.fn();
    render(
      <TabStrip
        tabs={[tab(1, "Alpha", 0), tab(2, "Beta", 1)]}
        activeId={1}
        onSelect={onSelect}
        onCreate={() => {}}
        onRename={() => {}}
        onClose={() => {}}
        onReorder={() => {}}
      />,
    );
    fireEvent.click(screen.getByRole("tab", { name: /Beta/ }));
    expect(onSelect).toHaveBeenCalledWith(2);
  });

  it("fires onCreate when clicking the + button", () => {
    const onCreate = vi.fn();
    render(
      <TabStrip
        tabs={[tab(1, "Alpha", 0)]}
        activeId={1}
        onSelect={() => {}}
        onCreate={onCreate}
        onRename={() => {}}
        onClose={() => {}}
        onReorder={() => {}}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /new tab|nova aba/i }));
    expect(onCreate).toHaveBeenCalled();
  });
});
