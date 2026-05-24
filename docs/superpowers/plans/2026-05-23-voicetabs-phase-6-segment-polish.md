# VoiceTabs — Phase 6 (Segment Polish + UX) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Phase 5 dependency.** This plan starts AFTER Phase 5 (hotkey + tray + capture modes) has landed on `master`. Phase 5 does not touch any of the files this plan modifies (`SegmentCard.tsx`, `TabBody.tsx`, `TabStrip.tsx`, `SettingsDrawer.tsx`, `VocabSettings.tsx`, `settingsStore.ts`), so the two plans are strictly serial — no merge gymnastics expected. If Phase 5 has introduced a new top-level layout shell that wraps `TabStrip`/`TabBody`, the tab-strip overflow CSS in Task 9 may need a sibling-selector tweak; pause and rebase if so. **Do not begin** while Phase 5's branch is still open.

> **Phase 7 follow-up.** Phase 7 (OpenAI cloud backend) is drafted in parallel with this plan and will land AFTER. Phase 7 touches `SttStatusDot`, `SttStore`, the worker side, and the settings drawer for a backend picker — none of which overlap with Phase 6's surface. They commute.

**Goal:** Turn the functional-but-bare segment UX from Phase 4 into something pleasant to live in. After Phase 6, every segment card shows the time it was spoken, every card has a one-click copy, the whole tab can be copied with one click, the list auto-scrolls intelligently (only when the user is near the bottom), the empty state is inviting, the tab strip handles overflow gracefully and surfaces a per-tab segment count, and the vocab editor (if scope allows) is a chip component instead of a textarea. One new settings key — `show_timestamps` (default `true`) — is the only persistence change.

**Architecture:** Frontend-only, no new Tauri commands, no new Rust code. Each component owns a single concern:

- `SegmentCard` gains a timestamp `<time>` element (rendered when `settings.show_timestamps`) and a hover-revealed "Copy" button. Both are pure additive rendering — existing tests stay green.
- `TabBody` gains intelligent auto-scroll: a `near-bottom` boolean tracked via `scroll`/`resize` events; when a new segment appears AND the user is near the bottom, smooth-scroll. When the user is scrolled up, surface a "Nova transcrição ↓" pill that scrolls on click. The empty-state placeholder is replaced with a centered hint card (only rendered after the first load completes — never during loading).
- `TabStrip` adds a CSS gradient fade at the horizontal-scroll edges and a small segment-count badge derived live from `segmentsStore`. Optional keyboard nav (Ctrl+Tab / Ctrl+Shift+Tab) lives in `App.tsx` as a global keydown listener so it works regardless of focus.
- `SettingsDrawer` gains a "Mostrar timestamps" checkbox. The new state in `settingsStore` is `showTimestamps: boolean` persisted under `settings.show_timestamps`.
- `VocabSettings` (if scope allows) flips from a `<textarea>` to a chip editor: existing terms render as removable chips, a text input adds new chips on Enter or comma. The store's `setVocabTerms` API is unchanged — only the editor view changes.

**Tech stack additions:** None. We rely on the existing Zustand + react-i18next stack. No new npm dependencies. Pure CSS for the gradient fade, plus a tiny `useNearBottom` hook (≤ 30 lines, no library).

**Reference spec:** `docs/superpowers/specs/2026-05-20-voicetabs-design.md` §7.12 (segment UI) and §8 (UI walkthrough — auto-scroll on new segment, subtle pulse on receiving tab). The Phase 4 plan explicitly defers timestamps + copy + drag-reorder + chip-editor to Phase 6; this plan picks them up except drag-reorder (still deferred).

**Builds on:** Phases 0+1 (`docs/superpowers/plans/2026-05-20-voicetabs-foundation.md`), Phase 2 (`docs/superpowers/plans/2026-05-20-voicetabs-phase-2-audio-vad.md`), Phase 3 (`docs/superpowers/plans/2026-05-22-voicetabs-phase-3-stt-subprocess.md`), Phase 4 (`docs/superpowers/plans/2026-05-22-voicetabs-phase-4-segments.md`), and Phase 5 (hotkey + tray, separate plan landing on `master` immediately before this one).

---

## Acceptance for this plan

- **Timestamp on every card.** With `show_timestamps = true` (default), each card renders a subtle `HH:MM:SS` derived from `segment.started_at` using `toLocaleTimeString(uiLocale, { hour12: false })`. Toggling "Mostrar timestamps" OFF in settings hides them across the entire list with no remount flicker. Setting persists across restart.
- **Copy segment.** Hovering a card reveals a copy icon next to play/edit. Click → `navigator.clipboard.writeText(segment.text)` + a 1.2 s "Copiado!" / "Copied!" inline confirmation that auto-dismisses. The button is keyboard-reachable (Tab) and has an `aria-label`.
- **Copy tab.** A new "Copy tab" button in the tab body header concatenates the active tab's segment texts with `"\n\n"` and writes to the clipboard. Same visual confirmation. Disabled when `segments.length === 0`.
- **Cross-segment selection.** Selecting from the middle of segment N through into segment N+1 and pressing Ctrl+C copies the concatenated text exactly as the user sees it. Verified manually in the acceptance step — no test, but the layout is CSS-only so the DOM's native range selection works.
- **Auto-scroll with intent detection.** When a new segment lands AND the scroll position is within 80 px of the bottom (`near-bottom` threshold), the list smooth-scrolls to the bottom. When the user is scrolled up beyond that threshold, no auto-scroll happens; instead a small "Nova transcrição ↓" / "New transcription ↓" pill appears anchored above the bottom of the viewport. Clicking the pill smooth-scrolls to bottom and dismisses the pill. The pill is also dismissed by the user manually scrolling back into the near-bottom zone.
- **Empty state.** Tabs with zero segments and `loading === false` render a centered hint card with a mic glyph + "Comece a falar para transcrever." / "Start speaking to transcribe." During `loading === true` (the brief window between switching tabs and `segmentsApi.listForTab` returning), the body is blank — not the empty state, not a spinner.
- **Tab strip overflow.** With ≥ 8 tabs visible at a default window width, the strip scrolls horizontally on wheel/swipe. A subtle dark-to-transparent gradient fade marks the left and right edges when content is clipped on that side; fade disappears when scrolled to the end. The active tab is brought into view via `scrollIntoView({ inline: "nearest" })` on selection change.
- **Segment count badge.** Each tab title shows a small badge with the count of segments for that tab, derived live from `useSegmentsStore`. The badge updates when segments are added, deleted, or arrive via the `segment-created` event. The badge is hidden for counts of 0 to keep the strip quiet for fresh tabs.
- **Keyboard nav (optional, behind feature flag pattern but unconditionally on):** Ctrl+Tab cycles to the next tab; Ctrl+Shift+Tab cycles to the previous. Wraps around. Ignored when focus is inside a textarea or input (we don't want to hijack while the user is editing a segment).
- **Vocab chips (optional within Phase 6).** If scope allows (Task 12 is marked **OPTIONAL** and may be skipped without blocking Phase 6 acceptance), the vocab editor renders existing terms as chips with a small "×" remover. A trailing `<input>` accepts free text and adds a chip on Enter or comma; backspace on empty input removes the last chip. Persistence path (`settingsApi.set("vocab_terms", JSON.stringify(...))`) is identical to v1.
- **All existing tests green.** `npm test` passes. `npx tsc --noEmit` clean. No Rust changes ⇒ no `cargo test` regression possible, but we still run it once at the end as a sanity check.
- **New TS tests:** ~22 new tests landing across SegmentCard (timestamp + copy), TabBody (auto-scroll near-bottom logic + empty-state-vs-loading + copy-tab button), TabStrip (count badge + overflow fade), settingsStore (`showTimestamps` persistence), and optionally VocabSettings chip editor (if Task 12 ships).

## Out of scope (deferred to later plans)

- **Drag-reorder of segments within a tab** — not in v1 at all. Segments stay append-only at `position = max+1`.
- **Cross-tab move of a segment** — same.
- **Search across segments** (Ctrl+F equivalent) — deferred to a future polish phase. The DOM-native browser find behaviour inside WebView2 is acceptable for v1.
- **Export tab content as `.txt` / `.md`** — explicit out-of-scope per the Phase 6 brief; "Copy tab" is the v1 export story.
- **Light/dark theme switch** — out. App stays on its current dark palette.
- **Hotkey + tray + capture modes** — Phase 5 (lands before this plan).
- **OpenAI cloud STT backend** — Phase 7 (lands after this plan).
- **Per-segment metadata pane** (logprob, model id) — not in v1; existing fields are kept on the row but not surfaced.
- **Pulse animation on receiving tab when a segment lands elsewhere** — spec §8 mentions a "subtle pulse" but Phase 6 brief does not list it as a deliverable. Deferred to a future polish pass (or fold into the badge count change as a CSS transition if cheap).

---

## File structure after this plan

```
src/
├── components/
│   ├── SegmentCard.tsx                   # MODIFIED: timestamp + copy button + "Copied!" feedback
│   ├── TabBody.tsx                       # MODIFIED: near-bottom auto-scroll, "new transcription" pill,
│   │                                     #           empty-state card (loading-aware), copy-tab button
│   ├── TabStrip.tsx                      # MODIFIED: per-tab segment count badge, overflow fade,
│   │                                     #           scroll-active-into-view on selection change
│   ├── SettingsDrawer.tsx                # MODIFIED: + "Mostrar timestamps" checkbox section
│   ├── VocabSettings.tsx                 # MODIFIED (optional Task 12): chip editor
│   └── EmptyTabHint.tsx                  # NEW: centered hint card (mic glyph + i18n text)
├── hooks/
│   └── useNearBottom.ts                  # NEW: tiny hook returning [ref, isNearBottom] given a threshold
├── stores/
│   └── settingsStore.ts                  # MODIFIED: + showTimestamps boolean + setter, load() reads key
├── lib/
│   └── clipboard.ts                      # NEW: thin wrapper around navigator.clipboard.writeText so
│                                         #      tests can spy without poking globals everywhere
├── i18n/locales/
│   ├── pt-BR.json                        # MODIFIED: + segments.copy, segments.copied, segments.copyTab,
│   │                                     #           segments.newTranscriptionPill, segments.emptyHint,
│   │                                     #           settings.showTimestamps,
│   │                                     #           (optional) settings.vocabAddChipHint
│   └── en.json                           # MODIFIED: mirror keys
├── styles.css                            # MODIFIED: timestamp typography, copy button hover-reveal,
│                                         #           empty-state card, tab-strip gradient fade,
│                                         #           segment-count badge, "new transcription" pill,
│                                         #           (optional) chip editor styles
├── App.tsx                                # MODIFIED: global Ctrl+Tab / Ctrl+Shift+Tab keydown listener
└── __tests__/
    ├── SegmentCard.test.tsx              # MODIFIED: + timestamp render + copy button + copied feedback
    ├── TabBody.test.tsx                  # NEW: near-bottom auto-scroll, pill render, empty-state gating,
    │                                     #      copy-tab button
    ├── TabStrip.test.tsx                 # MODIFIED: + segment-count badge + overflow fade present
    ├── SettingsDrawer.test.tsx           # NEW (or MODIFIED if Phase 5 added it): showTimestamps checkbox
    ├── settingsStore.test.tsx            # NEW: showTimestamps load/save round-trip
    ├── useNearBottom.test.tsx            # NEW: hook fires/clears across the threshold
    ├── VocabSettings.test.tsx            # MODIFIED (optional Task 12): chip editor behaviour
    └── setup.ts                          # unchanged (jsdom clipboard polyfill in clipboard.ts)
```

Each component still has one responsibility. `useNearBottom` is the only piece of shared logic and is small enough that two consumers (TabBody auto-scroll + pill visibility) share it without coupling. `clipboard.ts` exists purely to give tests a single seam — `vi.spyOn(clipboard, "writeText")` in any spec.

---

# Phase 6 tasks

## Task 1: Settings store — `showTimestamps` + drawer checkbox (TDD)

**Files:**
- Modify: `src/stores/settingsStore.ts`
- Modify: `src/components/SettingsDrawer.tsx`
- Modify: `src/i18n/locales/pt-BR.json`
- Modify: `src/i18n/locales/en.json`
- Create: `src/__tests__/settingsStore.test.tsx`
- Modify: `src/__tests__/i18n.test.tsx` (if it asserts on the drawer shape)

The Phase 4 store persists `ui_locale` and `vocab_terms` via the `settings` table. We follow the exact pattern for `show_timestamps`: read on `load()`, write on the setter, surface the boolean on the store. Default ON.

- [ ] **Step 1: Write the failing test in `src/__tests__/settingsStore.test.tsx`**

```tsx
import { describe, expect, it, vi, beforeEach } from "vitest";

const settingsGet = vi.fn();
const settingsSet = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args?: Record<string, unknown>) => {
    if (command === "settings_get") return settingsGet(args);
    if (command === "settings_set") return settingsSet(args);
    return undefined;
  }),
}));

const { useSettingsStore } = await import("../stores/settingsStore");

beforeEach(() => {
  useSettingsStore.setState({
    uiLocale: "en",
    drawerOpen: false,
    vocabTerms: [],
    showTimestamps: true,
  });
  settingsGet.mockReset();
  settingsSet.mockReset();
});

describe("settingsStore showTimestamps", () => {
  it("defaults to true when the key is absent", async () => {
    settingsGet.mockImplementation(async ({ key }: { key: string }) => {
      if (key === "show_timestamps") return null;
      return null;
    });
    await useSettingsStore.getState().load();
    expect(useSettingsStore.getState().showTimestamps).toBe(true);
  });

  it("reads false back as false", async () => {
    settingsGet.mockImplementation(async ({ key }: { key: string }) => {
      if (key === "show_timestamps") return "false";
      return null;
    });
    await useSettingsStore.getState().load();
    expect(useSettingsStore.getState().showTimestamps).toBe(false);
  });

  it("setShowTimestamps(false) persists the string 'false' and updates state", async () => {
    await useSettingsStore.getState().setShowTimestamps(false);
    expect(settingsSet).toHaveBeenCalledWith({ key: "show_timestamps", value: "false" });
    expect(useSettingsStore.getState().showTimestamps).toBe(false);
  });

  it("setShowTimestamps(true) persists the string 'true'", async () => {
    await useSettingsStore.getState().setShowTimestamps(true);
    expect(settingsSet).toHaveBeenCalledWith({ key: "show_timestamps", value: "true" });
    expect(useSettingsStore.getState().showTimestamps).toBe(true);
  });
});
```

- [ ] **Step 2: Extend `src/stores/settingsStore.ts`**

Add the const + state + load + setter. Encode as a literal `"true"` / `"false"` string for forward-compat with future enum values (we don't want to over-engineer with JSON for a boolean).

```ts
const TIMESTAMPS_KEY = "show_timestamps";

// inside SettingsState:
showTimestamps: boolean;
setShowTimestamps: (value: boolean) => Promise<void>;

// initial state:
showTimestamps: true,

// inside load() — add after the vocab block:
const tsRaw = await settingsApi.get(TIMESTAMPS_KEY);
const showTimestamps = tsRaw === null ? true : tsRaw !== "false";
set({ uiLocale, vocabTerms, showTimestamps });

// new setter:
async setShowTimestamps(value) {
  await settingsApi.set(TIMESTAMPS_KEY, value ? "true" : "false");
  set({ showTimestamps: value });
},
```

The existing `set({ uiLocale, vocabTerms })` call at the end of `load()` is replaced by the version that also carries `showTimestamps` so we still do one render.

- [ ] **Step 3: Add the i18n keys to `src/i18n/locales/pt-BR.json`**

Inside the existing `"settings"` block, after `"vocabPlaceholder"`:

```json
    "showTimestamps": "Mostrar timestamps",
    "showTimestampsHint": "Mostra a hora em cada cartão de segmento."
```

- [ ] **Step 4: Mirror in `src/i18n/locales/en.json`**

```json
    "showTimestamps": "Show timestamps",
    "showTimestampsHint": "Show the time on each segment card."
```

- [ ] **Step 5: Render the checkbox in `src/components/SettingsDrawer.tsx`**

Add a new section between the language `<section>` and `<VocabSettings />`:

```tsx
const showTimestamps = useSettingsStore((s) => s.showTimestamps);
const setShowTimestamps = useSettingsStore((s) => s.setShowTimestamps);

// ...

<section className="drawer__section">
  <label htmlFor="show-timestamps-toggle">
    <input
      id="show-timestamps-toggle"
      type="checkbox"
      checked={showTimestamps}
      onChange={(e) => void setShowTimestamps(e.target.checked)}
    />
    {t("settings.showTimestamps")}
  </label>
  <small className="drawer__hint">{t("settings.showTimestampsHint")}</small>
</section>
```

- [ ] **Step 6: Run the tests**

```powershell
npm test -- settingsStore
```

Expected: 4 new tests pass. Existing tests stay green.

- [ ] **Step 7: Commit**

```powershell
git add src/stores/settingsStore.ts src/components/SettingsDrawer.tsx src/i18n src/__tests__/settingsStore.test.tsx
git commit -m "feat(settings): show_timestamps boolean + drawer toggle"
```

---

## Task 2: Clipboard helper

**Files:**
- Create: `src/lib/clipboard.ts`

A one-function module so SegmentCard, TabBody, and any future copy consumer go through a single seam that tests can spy on without touching `navigator`.

- [ ] **Step 1: Create `src/lib/clipboard.ts`**

```ts
/**
 * Thin wrapper around `navigator.clipboard.writeText` so callers and tests
 * can mock a single export instead of poking the global navigator. In
 * Tauri's WebView2 the Clipboard API is available without explicit permission
 * prompts; we still fall back to a no-op + console.warn if it's missing
 * (older WebView2 runtimes or jsdom without polyfill).
 */
export async function writeText(text: string): Promise<void> {
  if (navigator.clipboard && typeof navigator.clipboard.writeText === "function") {
    await navigator.clipboard.writeText(text);
    return;
  }
  console.warn("clipboard.writeText: navigator.clipboard unavailable");
}
```

- [ ] **Step 2: Commit**

```powershell
git add src/lib/clipboard.ts
git commit -m "feat(lib): clipboard.writeText wrapper for test seam"
```

---

## Task 3: SegmentCard — timestamp rendering (TDD)

**Files:**
- Modify: `src/components/SegmentCard.tsx`
- Modify: `src/__tests__/SegmentCard.test.tsx`

We render a `<time>` element with the user's locale's `HH:MM:SS` derived from `segment.started_at` (unix ms). Visibility is gated by `settings.showTimestamps`. The element uses an ISO `dateTime` attribute for accessibility / future copy-with-context.

- [ ] **Step 1: Append the test in `src/__tests__/SegmentCard.test.tsx`**

The existing test file mocks `convertFileSrc`. The store is not directly imported by `SegmentCard` — we pass `showTimestamps` as a prop to keep the component dumb and easily testable.

Add tests at the end of the existing `describe("SegmentCard", ...)` block:

```tsx
it("renders the segment timestamp as HH:MM:SS when showTimestamps is true", () => {
  // 2026-05-23 14:07:42 UTC -> in the test env we render the local time, so
  // we don't assert on a literal string. We assert on the <time> element's
  // presence and on the format shape.
  const seg = mkSegment("hi");
  seg.started_at = Date.UTC(2026, 4, 23, 14, 7, 42); // May = month 4
  render(
    <SegmentCard
      segment={seg}
      onEdit={noop}
      onDelete={noop}
      onRetranscribe={noop}
      showTimestamps
    />,
  );
  const el = screen.getByRole("time", { hidden: true }) ?? screen.getByText(/\d{2}:\d{2}:\d{2}/);
  // Browser's <time> doesn't have an implicit ARIA role; query the tag directly.
  const tag = document.querySelector("time");
  expect(tag).not.toBeNull();
  expect(tag!.getAttribute("datetime")).toBe(new Date(seg.started_at).toISOString());
  expect(tag!.textContent).toMatch(/^\d{2}:\d{2}:\d{2}$/);
});

it("omits the timestamp when showTimestamps is false", () => {
  render(
    <SegmentCard
      segment={mkSegment("hi")}
      onEdit={noop}
      onDelete={noop}
      onRetranscribe={noop}
      showTimestamps={false}
    />,
  );
  expect(document.querySelector("time")).toBeNull();
});

it("defaults to NOT rendering the timestamp when prop is omitted", () => {
  // Forward-compat: callers that haven't been updated to pass showTimestamps
  // get the safe behaviour (no timestamp) rather than a surprise UI change.
  render(
    <SegmentCard
      segment={mkSegment("hi")}
      onEdit={noop}
      onDelete={noop}
      onRetranscribe={noop}
    />,
  );
  expect(document.querySelector("time")).toBeNull();
});
```

- [ ] **Step 2: Extend the Props + render in `src/components/SegmentCard.tsx`**

Add `showTimestamps?: boolean` (default `false` — the parent passes the live value). Inside `segment-card__body`, render a `<time>` above the paragraph when the prop is true and we're not in edit mode:

```tsx
type Props = {
  segment: Segment;
  onEdit: (id: number, newText: string) => void;
  onDelete: (id: number) => void;
  onRetranscribe: (id: number, mode: RetranscribeMode) => void;
  isTranscribing?: boolean;
  showTimestamps?: boolean;
};

// at top of body, before the editing/transcribing/text ternary:
{showTimestamps && !editing && (
  <time
    className="segment-card__timestamp"
    dateTime={new Date(segment.started_at).toISOString()}
  >
    {formatTimestamp(segment.started_at)}
  </time>
)}
```

And the helper at the bottom of the file, alongside `audioDirHint`:

```ts
/**
 * Format a unix-ms timestamp as HH:MM:SS in the user's locale, 24-hour
 * clock. We avoid `Date.toString` because that's implementation-defined and
 * leaks the timezone abbreviation; `toLocaleTimeString` with `hour12: false`
 * gives us a clean `HH:MM:SS`. The user's locale is whatever the browser/
 * Tauri WebView2 reports — react-i18next's current locale matches because
 * `document.documentElement.lang` is set on locale change.
 */
function formatTimestamp(unixMs: number): string {
  const d = new Date(unixMs);
  return d.toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  });
}
```

- [ ] **Step 3: Append the CSS to `src/styles.css`**

```css
.segment-card__timestamp {
  display: block;
  color: #888;
  font-size: 11px;
  font-variant-numeric: tabular-nums;
  margin-bottom: 4px;
  user-select: none;
}
```

`user-select: none` is intentional — when the user drags across cards to copy text, we don't want the timestamp pulled in.

- [ ] **Step 4: Run the tests**

```powershell
npm test -- SegmentCard
```

Expected: existing tests + 3 new = ~11 pass.

- [ ] **Step 5: Commit**

```powershell
git add src/components/SegmentCard.tsx src/styles.css src/__tests__/SegmentCard.test.tsx
git commit -m "feat(SegmentCard): timestamp render gated on showTimestamps prop"
```

---

## Task 4: SegmentCard — copy button (TDD)

**Files:**
- Modify: `src/components/SegmentCard.tsx`
- Modify: `src/__tests__/SegmentCard.test.tsx`
- Modify: `src/i18n/locales/pt-BR.json` + `src/i18n/locales/en.json`

The copy button sits between play and edit. Hover reveals it (same as play/edit today). Click → `clipboard.writeText(segment.text)` → flash an inline "Copiado!" / "Copied!" span for 1.2 s. The flash auto-dismisses via `setTimeout`; we clear the timer on unmount.

- [ ] **Step 1: Add the i18n keys**

`pt-BR.json` `"segments"` block:

```json
    "copy": "Copiar segmento",
    "copied": "Copiado!"
```

`en.json`:

```json
    "copy": "Copy segment",
    "copied": "Copied!"
```

- [ ] **Step 2: Write the failing tests in `src/__tests__/SegmentCard.test.tsx`**

Add a mock for the clipboard module at the top of the file, before the `SegmentCard` import:

```tsx
vi.mock("../lib/clipboard", () => ({
  writeText: vi.fn(async () => {}),
}));
```

Then at the bottom of the `describe`:

```tsx
import { writeText as clipboardWriteText } from "../lib/clipboard";

// ...

it("Copy button writes segment.text to the clipboard", async () => {
  render(
    <SegmentCard
      segment={mkSegment("hello clipboard")}
      onEdit={noop}
      onDelete={noop}
      onRetranscribe={noop}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: /copy|copiar/i }));
  expect(clipboardWriteText).toHaveBeenCalledWith("hello clipboard");
});

it("Copy button shows the 'Copied!' confirmation, then hides it", async () => {
  vi.useFakeTimers();
  render(
    <SegmentCard
      segment={mkSegment("x")}
      onEdit={noop}
      onDelete={noop}
      onRetranscribe={noop}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: /copy|copiar/i }));
  expect(await screen.findByText(/copied!|copiado!/i)).toBeInTheDocument();
  vi.advanceTimersByTime(1300);
  expect(screen.queryByText(/copied!|copiado!/i)).toBeNull();
  vi.useRealTimers();
});
```

- [ ] **Step 3: Extend `src/components/SegmentCard.tsx`**

Add the state + handler + button, plus the cleanup effect:

```tsx
import { writeText } from "../lib/clipboard";

// inside the component:
const [justCopied, setJustCopied] = useState(false);
const copyTimerRef = useRef<number | null>(null);

useEffect(() => {
  return () => {
    if (copyTimerRef.current !== null) window.clearTimeout(copyTimerRef.current);
  };
}, []);

async function handleCopy() {
  await writeText(segment.text);
  setJustCopied(true);
  if (copyTimerRef.current !== null) window.clearTimeout(copyTimerRef.current);
  copyTimerRef.current = window.setTimeout(() => setJustCopied(false), 1200);
}

// in the actions toolbar, between play and edit:
<button
  className="segment-card__copy"
  aria-label={t("segments.copy")}
  onClick={() => void handleCopy()}
>
  {"⧉"}
</button>
{justCopied && (
  <span className="segment-card__copied" role="status" aria-live="polite">
    {t("segments.copied")}
  </span>
)}
```

Order in the toolbar: play, copy, edit, overflow. Hover-reveal styling matches the existing pattern.

- [ ] **Step 4: Append CSS to `src/styles.css`**

```css
.segment-card__copy {
  opacity: 0;
  transition: opacity 120ms ease;
}
.segment-card:hover .segment-card__copy,
.segment-card:focus-within .segment-card__copy {
  opacity: 1;
}
.segment-card__copied {
  font-size: 11px;
  color: #6bd16b;
  margin-left: 8px;
  animation: fadeout 1200ms ease forwards;
}
@keyframes fadeout {
  0%, 70% { opacity: 1; }
  100%    { opacity: 0; }
}
```

- [ ] **Step 5: Run the tests**

```powershell
npm test -- SegmentCard
```

Expected: 13 total SegmentCard tests pass.

- [ ] **Step 6: Commit**

```powershell
git add src/components/SegmentCard.tsx src/styles.css src/__tests__/SegmentCard.test.tsx src/i18n
git commit -m "feat(SegmentCard): hover-revealed copy button with inline confirmation"
```

---

## Task 5: `useNearBottom` hook (TDD)

**Files:**
- Create: `src/hooks/useNearBottom.ts`
- Create: `src/__tests__/useNearBottom.test.tsx`

A tiny hook: returns `[scrollContainerRef, isNearBottom]`. Recomputes on `scroll` events and on `ResizeObserver` callbacks for the container. Threshold defaults to 80 px. Cleans up on unmount.

We test by directly mutating `scrollTop`/`scrollHeight`/`clientHeight` and dispatching a synthetic `scroll` event — jsdom supports this. ResizeObserver isn't in jsdom by default; we install a tiny polyfill inside the hook's defensive feature check (no-op if missing) and don't test the ResizeObserver path in unit tests (covered by manual acceptance).

- [ ] **Step 1: Write the failing test in `src/__tests__/useNearBottom.test.tsx`**

```tsx
import { renderHook, act } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { useNearBottom } from "../hooks/useNearBottom";

function makeContainer(scrollTop: number, scrollHeight: number, clientHeight: number) {
  const div = document.createElement("div");
  Object.defineProperty(div, "scrollTop",    { value: scrollTop,    writable: true });
  Object.defineProperty(div, "scrollHeight", { value: scrollHeight, writable: true });
  Object.defineProperty(div, "clientHeight", { value: clientHeight, writable: true });
  return div;
}

describe("useNearBottom", () => {
  it("reports near-bottom when distance to bottom is under threshold", () => {
    const { result } = renderHook(() => useNearBottom(80));
    const div = makeContainer(900, 1000, 100); // bottom: 1000, viewport bottom at 1000, dist 0
    act(() => result.current[0](div));
    act(() => div.dispatchEvent(new Event("scroll")));
    expect(result.current[1]).toBe(true);
  });

  it("reports not-near-bottom when scrolled up beyond the threshold", () => {
    const { result } = renderHook(() => useNearBottom(80));
    const div = makeContainer(0, 1000, 100); // dist = 900
    act(() => result.current[0](div));
    act(() => div.dispatchEvent(new Event("scroll")));
    expect(result.current[1]).toBe(false);
  });

  it("re-evaluates when scrollTop changes via subsequent scroll event", () => {
    const { result } = renderHook(() => useNearBottom(80));
    const div = makeContainer(0, 1000, 100);
    act(() => result.current[0](div));
    act(() => div.dispatchEvent(new Event("scroll")));
    expect(result.current[1]).toBe(false);
    (div as any).scrollTop = 920;
    act(() => div.dispatchEvent(new Event("scroll")));
    expect(result.current[1]).toBe(true);
  });
});
```

- [ ] **Step 2: Create `src/hooks/useNearBottom.ts`**

```ts
import { useCallback, useEffect, useRef, useState } from "react";

/**
 * Tracks whether the attached scroll container is currently within
 * `threshold` pixels of its bottom edge.
 *
 * Usage:
 *   const [setRef, isNearBottom] = useNearBottom(80);
 *   <div ref={setRef}>…</div>
 *
 * We intentionally use a callback ref (not a `useRef<HTMLElement>`) so
 * React calls us when the element mounts/unmounts, and we can attach/
 * detach listeners exactly once. `ResizeObserver` covers the case where
 * the content height changes (e.g. an image inside a segment finishes
 * loading); we feature-check it because jsdom does not implement it.
 */
export function useNearBottom(
  threshold = 80,
): [(el: HTMLElement | null) => void, boolean] {
  const [near, setNear] = useState(true);
  const elRef = useRef<HTMLElement | null>(null);

  const compute = useCallback(() => {
    const el = elRef.current;
    if (!el) return;
    const distance = el.scrollHeight - el.scrollTop - el.clientHeight;
    setNear(distance <= threshold);
  }, [threshold]);

  const setRef = useCallback(
    (el: HTMLElement | null) => {
      // Detach from previous.
      const prev = elRef.current;
      if (prev) {
        prev.removeEventListener("scroll", compute);
      }
      elRef.current = el;
      if (el) {
        el.addEventListener("scroll", compute, { passive: true });
        compute();
      }
    },
    [compute],
  );

  useEffect(() => {
    const el = elRef.current;
    if (!el) return;
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(() => compute());
    ro.observe(el);
    return () => ro.disconnect();
  }, [compute]);

  return [setRef, near];
}
```

- [ ] **Step 3: Run the tests**

```powershell
npm test -- useNearBottom
```

Expected: 3 new tests pass.

- [ ] **Step 4: Commit**

```powershell
git add src/hooks/useNearBottom.ts src/__tests__/useNearBottom.test.tsx
git commit -m "feat(hooks): useNearBottom — track scroll distance from bottom"
```

---

## Task 6: TabBody — intelligent auto-scroll + "new transcription" pill (TDD)

**Files:**
- Modify: `src/components/TabBody.tsx`
- Create: `src/__tests__/TabBody.test.tsx`
- Modify: `src/styles.css`
- Modify: `src/i18n/locales/pt-BR.json` + `src/i18n/locales/en.json`

Today's TabBody scrolls to bottom on every `segments.length` change via `scrollIntoView`. That's the wrong UX when the user is reading older content. Replace with: scroll-to-bottom iff `isNearBottom`; otherwise set `pendingNewSegments = true` and render the pill.

The pill click invokes the same scroll-to-bottom; the pill auto-hides when `isNearBottom` flips back to `true`.

- [ ] **Step 1: Add the i18n keys**

`pt-BR.json` `"segments"`:

```json
    "newTranscriptionPill": "Nova transcrição ↓"
```

`en.json` `"segments"`:

```json
    "newTranscriptionPill": "New transcription ↓"
```

- [ ] **Step 2: Write the failing tests in `src/__tests__/TabBody.test.tsx`**

We mock the store and the API; we don't need a real Tauri backend.

```tsx
import { fireEvent, render, screen, act } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => []),
  convertFileSrc: (p: string) => `asset://localhost/${encodeURIComponent(p)}`,
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));
vi.mock("../lib/clipboard", () => ({
  writeText: vi.fn(async () => {}),
}));

const { TabBody } = await import("../components/TabBody");
const { useSegmentsStore } = await import("../stores/segmentsStore");

function seg(id: number, text = `s${id}`) {
  return {
    id, tab_id: 1, position: id, text, original_text: text, audio_path: `${id}.wav`,
    started_at: id * 1_000, ended_at: id * 1_000 + 500, duration_ms: 500,
    vocab_snapshot: "[]", avg_logprob: -0.3, no_speech_prob: 0.02, model_id: "m",
  };
}

beforeEach(() => {
  useSegmentsStore.setState({
    segmentsByTab: {},
    loading: {},
    unlistenSegmentCreated: null,
  });
});

describe("TabBody", () => {
  it("renders the empty-state card when not loading AND segments.length === 0", () => {
    useSegmentsStore.setState({ segmentsByTab: { 1: [] }, loading: { 1: false } });
    render(<TabBody activeTabId={1} />);
    expect(screen.getByText(/comece a falar|start speaking/i)).toBeInTheDocument();
  });

  it("does NOT render the empty-state card while loading is true", () => {
    useSegmentsStore.setState({ segmentsByTab: {}, loading: { 1: true } });
    render(<TabBody activeTabId={1} />);
    expect(screen.queryByText(/comece a falar|start speaking/i)).toBeNull();
  });

  it("renders the Copy tab button and writes joined segments to clipboard", async () => {
    const { writeText } = await import("../lib/clipboard");
    useSegmentsStore.setState({
      segmentsByTab: { 1: [seg(1, "first"), seg(2, "second")] },
      loading: { 1: false },
    });
    render(<TabBody activeTabId={1} />);
    fireEvent.click(screen.getByRole("button", { name: /copy tab|copiar aba/i }));
    expect(writeText).toHaveBeenCalledWith("first\n\nsecond");
  });

  it("shows the 'new transcription' pill when scrolled up and a segment arrives", async () => {
    useSegmentsStore.setState({
      segmentsByTab: { 1: [seg(1)] },
      loading: { 1: false },
    });
    const { rerender } = render(<TabBody activeTabId={1} />);
    // Force the scroll container into "not near bottom" state.
    const list = document.querySelector(".tab-body__list") as HTMLDivElement;
    Object.defineProperty(list, "scrollTop",    { value: 0,    writable: true });
    Object.defineProperty(list, "scrollHeight", { value: 5000, writable: true });
    Object.defineProperty(list, "clientHeight", { value: 200,  writable: true });
    act(() => list.dispatchEvent(new Event("scroll")));
    // New segment lands.
    act(() => {
      useSegmentsStore.setState({
        segmentsByTab: { 1: [seg(1), seg(2)] },
        loading: { 1: false },
      });
    });
    rerender(<TabBody activeTabId={1} />);
    expect(screen.getByRole("button", { name: /new transcription|nova transcrição/i }))
      .toBeInTheDocument();
  });
});
```

- [ ] **Step 3: Rewrite the relevant parts of `src/components/TabBody.tsx`**

Key changes:
- Move the scroll container to its own `<div ref={setNearBottomRef} className="tab-body__list">` so the hook attaches to the actual scroller.
- Track `prevSegmentCountRef` (a `useRef<number>`) to detect "new segment arrived" without scheduling effects per render.
- On `segments.length` increasing and `isNearBottom === true` → call `scrollContainer.scrollTo({ top: scrollHeight, behavior: "smooth" })`.
- On `segments.length` increasing and `isNearBottom === false` → `setPendingNew(true)`.
- When `isNearBottom` flips to `true`, `setPendingNew(false)`.
- Add a "Copy tab" button in a new `<header className="tab-body__header">` placed above the list.
- Replace the inline `<p>` empty state with `<EmptyTabHint />` rendered only when `segments.length === 0 && loading === false`.

```tsx
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
  const segmentsByTab    = useSegmentsStore((s) => s.segmentsByTab);
  const loadingByTab     = useSegmentsStore((s) => s.loading);
  const loadForTab       = useSegmentsStore((s) => s.loadForTab);
  const updateSegmentText= useSegmentsStore((s) => s.updateSegmentText);
  const deleteSegment    = useSegmentsStore((s) => s.deleteSegment);
  const applyRetranscribe= useSegmentsStore((s) => s.applyRetranscribe);
  const showTimestamps   = useSettingsStore((s) => s.showTimestamps);

  const [transcribingId, setTranscribingId] = useState<number | null>(null);
  const [pendingNew, setPendingNew] = useState(false);

  const segments: Segment[] = activeTabId == null ? [] : segmentsByTab[activeTabId] ?? [];
  const loading = activeTabId == null ? false : !!loadingByTab[activeTabId];

  useEffect(() => {
    if (activeTabId != null) void loadForTab(activeTabId);
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

  if (activeTabId == null) return null;

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
        <button
          className="tab-body__pill"
          onClick={scrollToBottom}
        >
          {t("segments.newTranscriptionPill")}
        </button>
      )}
    </div>
  );
}
```

- [ ] **Step 4: Add `segments.copyTab` i18n keys**

`pt-BR.json` `"segments"`:

```json
    "copyTab": "Copiar aba"
```

`en.json` `"segments"`:

```json
    "copyTab": "Copy tab"
```

- [ ] **Step 5: Append the CSS to `src/styles.css`**

```css
.tab-body {
  display: flex;
  flex-direction: column;
  height: 100%;
  position: relative;
}
.tab-body__header {
  display: flex;
  justify-content: flex-end;
  padding: 6px 12px;
  border-bottom: 1px solid #2a2a2a;
}
.tab-body__copy-tab {
  background: transparent;
  color: #cfcfcf;
  border: 1px solid #3a3a3a;
  padding: 4px 10px;
  font-size: 12px;
  cursor: pointer;
}
.tab-body__copy-tab:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}
.tab-body__list {
  flex: 1;
  overflow-y: auto;
  /* Keep new content anchored when content is appended at the bottom. */
  overflow-anchor: auto;
  padding: 12px;
}
.tab-body__pill {
  position: absolute;
  bottom: 16px;
  left: 50%;
  transform: translateX(-50%);
  background: #2563eb;
  color: #fff;
  border: none;
  border-radius: 999px;
  padding: 6px 14px;
  font-size: 12px;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.35);
  cursor: pointer;
}
```

- [ ] **Step 6: Run the tests**

```powershell
npm test -- TabBody
```

Expected: 4 new tests pass.

- [ ] **Step 7: Commit**

```powershell
git add src/components/TabBody.tsx src/styles.css src/__tests__/TabBody.test.tsx src/i18n
git commit -m "feat(TabBody): near-bottom auto-scroll, new-transcription pill, copy-tab button"
```

---

## Task 7: Empty-state hint component

**Files:**
- Create: `src/components/EmptyTabHint.tsx`
- Modify: `src/i18n/locales/pt-BR.json` + `src/i18n/locales/en.json`
- Modify: `src/styles.css`

Tiny, presentation-only component. No test of its own — TabBody's test asserts on its presence.

- [ ] **Step 1: Add the i18n key**

`pt-BR.json` `"segments"`:

```json
    "emptyHint": "Comece a falar para transcrever."
```

`en.json` `"segments"`:

```json
    "emptyHint": "Start speaking to transcribe."
```

(The existing `"emptyState"` key from Phase 4 — `"Fale algo para começar."` — stays as a fallback in case any caller still references it; remove it in a later cleanup pass once nothing imports it.)

- [ ] **Step 2: Create `src/components/EmptyTabHint.tsx`**

```tsx
import { useTranslation } from "react-i18next";

export function EmptyTabHint() {
  const { t } = useTranslation();
  return (
    <div className="empty-tab-hint" role="status" aria-live="polite">
      <div className="empty-tab-hint__icon" aria-hidden>🎤</div>
      <p className="empty-tab-hint__text">{t("segments.emptyHint")}</p>
    </div>
  );
}
```

We use the mic emoji as the glyph for v1; replacing it with an SVG later is a one-line change.

- [ ] **Step 3: Append the CSS**

```css
.empty-tab-hint {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  color: #aaa;
  gap: 8px;
  padding: 32px;
  text-align: center;
}
.empty-tab-hint__icon {
  font-size: 48px;
  opacity: 0.7;
}
.empty-tab-hint__text {
  font-size: 14px;
  max-width: 280px;
}
```

- [ ] **Step 4: Commit**

```powershell
git add src/components/EmptyTabHint.tsx src/styles.css src/i18n
git commit -m "feat(EmptyTabHint): centered hint card for empty tabs"
```

---

## Task 8: TabStrip — segment count badges (TDD)

**Files:**
- Modify: `src/components/TabStrip.tsx`
- Modify: `src/__tests__/TabStrip.test.tsx`
- Modify: `src/styles.css`

Each tab title shows a small badge with the segment count for that tab, derived from `useSegmentsStore`. Hidden when count is 0. Updates live because Zustand subscribes the component to `segmentsByTab` changes.

Important: the badge must derive from the store, not from a separate state — that's a self-review item from the brief.

- [ ] **Step 1: Write the failing test in `src/__tests__/TabStrip.test.tsx`**

Add to the existing TabStrip tests (or create the test file if it doesn't yet test segment counts):

```tsx
import { useSegmentsStore } from "../stores/segmentsStore";

// inside describe:
it("renders a segment count badge next to tabs with segments", () => {
  useSegmentsStore.setState({
    segmentsByTab: {
      1: [/* 3 stub segments */] as any,
      2: [] as any,
    },
    loading: {},
    unlistenSegmentCreated: null,
  });
  // Stub-fill tab 1 with 3 items.
  useSegmentsStore.setState({
    segmentsByTab: { 1: [stub(1), stub(2), stub(3)], 2: [] },
    loading: {}, unlistenSegmentCreated: null,
  });
  render(
    <TabStrip
      tabs={[{ id: 1, title: "A", order_idx: 0 } as any, { id: 2, title: "B", order_idx: 1 } as any]}
      activeId={1}
      onSelect={() => {}}
      onCreate={() => {}}
      onRename={() => {}}
      onClose={() => {}}
      onReorder={() => {}}
    />,
  );
  expect(screen.getByText("3")).toBeInTheDocument();
  // Tab B has 0 segments → no badge text.
  expect(screen.queryByText("0")).toBeNull();
});

function stub(id: number) {
  return {
    id, tab_id: 1, position: id, text: "x", original_text: "x", audio_path: `${id}.wav`,
    started_at: 0, ended_at: 0, duration_ms: 0, vocab_snapshot: "[]",
    avg_logprob: 0, no_speech_prob: 0, model_id: "m",
  };
}
```

- [ ] **Step 2: Render the badge in `src/components/TabStrip.tsx`**

Inside `SortableTab`, add the badge after the title span:

```tsx
{props.segmentCount > 0 && (
  <span className="tab__count" aria-label={`${props.segmentCount} segments`}>
    {props.segmentCount}
  </span>
)}
```

Add `segmentCount: number` to `SortableTab`'s props.

In the `TabStrip` function, subscribe to the store and pass the count in:

```tsx
const segmentsByTab = useSegmentsStore((s) => s.segmentsByTab);
// ...
<SortableTab
  // ... existing props
  segmentCount={(segmentsByTab[tab.id] ?? []).length}
/>
```

- [ ] **Step 3: Append CSS**

```css
.tab__count {
  display: inline-block;
  background: #2a2a2a;
  color: #cfcfcf;
  border-radius: 10px;
  font-size: 10px;
  padding: 1px 6px;
  margin-left: 6px;
  font-variant-numeric: tabular-nums;
}
.tab--active .tab__count {
  background: #3b82f6;
  color: #fff;
}
```

- [ ] **Step 4: Run the tests**

```powershell
npm test -- TabStrip
```

Expected: existing tests + new badge test pass.

- [ ] **Step 5: Commit**

```powershell
git add src/components/TabStrip.tsx src/styles.css src/__tests__/TabStrip.test.tsx
git commit -m "feat(TabStrip): per-tab segment count badge derived from segmentsStore"
```

---

## Task 9: TabStrip — overflow fade + scroll-active-into-view

**Files:**
- Modify: `src/components/TabStrip.tsx`
- Modify: `src/styles.css`

The strip already lays out tabs in a flexbox row. We give the container `overflow-x: auto` and add `::before` / `::after` pseudo-elements (or sibling overlay divs) for the gradient fade. We show/hide each fade based on `scrollLeft` and `scrollLeft + clientWidth < scrollWidth` using a tiny effect.

We also call `scrollIntoView({ inline: "nearest" })` on the active tab when `activeId` changes so the active tab is always visible after Ctrl+Tab cycling (Task 10) or programmatic selection.

- [ ] **Step 1: Wrap the strip in an overflow container**

```tsx
const stripRef = useRef<HTMLDivElement | null>(null);
const [leftFade, setLeftFade] = useState(false);
const [rightFade, setRightFade] = useState(false);

useEffect(() => {
  const el = stripRef.current;
  if (!el) return;
  const recompute = () => {
    setLeftFade(el.scrollLeft > 4);
    setRightFade(el.scrollLeft + el.clientWidth < el.scrollWidth - 4);
  };
  recompute();
  el.addEventListener("scroll", recompute, { passive: true });
  const ro = typeof ResizeObserver !== "undefined" ? new ResizeObserver(recompute) : null;
  ro?.observe(el);
  return () => {
    el.removeEventListener("scroll", recompute);
    ro?.disconnect();
  };
}, [tabs.length]);

// Scroll the active tab into view when selection changes.
useEffect(() => {
  const el = stripRef.current?.querySelector(`[data-tab-id="${activeId}"]`);
  if (el && "scrollIntoView" in el) {
    (el as HTMLElement).scrollIntoView({ inline: "nearest", block: "nearest" });
  }
}, [activeId]);
```

Wrap the `<div className="tab-strip">` in `<div className="tab-strip-wrap">` with conditional `--fade-left` / `--fade-right` class names, and add `data-tab-id={tab.id}` to each `SortableTab`'s root.

- [ ] **Step 2: CSS for the fades**

```css
.tab-strip-wrap {
  position: relative;
}
.tab-strip {
  overflow-x: auto;
  scrollbar-width: thin;
}
.tab-strip::-webkit-scrollbar { height: 6px; }
.tab-strip-wrap::before,
.tab-strip-wrap::after {
  content: "";
  position: absolute;
  top: 0;
  bottom: 0;
  width: 24px;
  pointer-events: none;
  opacity: 0;
  transition: opacity 120ms ease;
}
.tab-strip-wrap::before {
  left: 0;
  background: linear-gradient(to right, #1a1a1a, transparent);
}
.tab-strip-wrap::after {
  right: 0;
  background: linear-gradient(to left,  #1a1a1a, transparent);
}
.tab-strip-wrap.tab-strip-wrap--fade-left::before  { opacity: 1; }
.tab-strip-wrap.tab-strip-wrap--fade-right::after  { opacity: 1; }
```

(Adjust the gradient endpoint colour to match the actual strip background; `#1a1a1a` is a reasonable guess matching the existing dark palette.)

- [ ] **Step 3: Light test for fade element presence**

In `src/__tests__/TabStrip.test.tsx`:

```tsx
it("wraps the strip in a fade container", () => {
  render(<TabStrip {/* …props… */} />);
  expect(document.querySelector(".tab-strip-wrap")).not.toBeNull();
});
```

We don't try to assert on the dynamic fade classes in jsdom (no real scrollWidth). Acceptance is manual: shrink the window until tabs overflow → fades appear.

- [ ] **Step 4: Run tests**

```powershell
npm test -- TabStrip
```

- [ ] **Step 5: Commit**

```powershell
git add src/components/TabStrip.tsx src/styles.css src/__tests__/TabStrip.test.tsx
git commit -m "feat(TabStrip): overflow gradient fade + scroll-active-into-view"
```

---

## Task 10: Keyboard nav — Ctrl+Tab / Ctrl+Shift+Tab in `App.tsx`

**Files:**
- Modify: `src/App.tsx`

Global `keydown` listener cycles tabs. Ignored when the active element is an `<input>`, `<textarea>`, or `contenteditable` — we never want to hijack from a user mid-edit.

- [ ] **Step 1: Add the effect to `src/App.tsx`**

```tsx
import { useTabsStore } from "./stores/tabsStore";

// inside App():
const tabs = useTabsStore((s) => s.tabs);
const activeTabId = useTabsStore((s) => s.activeTabId);
const setActive = useTabsStore((s) => s.setActive);

useEffect(() => {
  function onKeyDown(e: KeyboardEvent) {
    if (!e.ctrlKey || e.key !== "Tab") return;
    const target = e.target as HTMLElement | null;
    const tag = target?.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || target?.isContentEditable) return;
    if (tabs.length === 0 || activeTabId == null) return;
    e.preventDefault();
    const idx = tabs.findIndex((t) => t.id === activeTabId);
    if (idx < 0) return;
    const delta = e.shiftKey ? -1 : 1;
    const next = tabs[(idx + delta + tabs.length) % tabs.length];
    void setActive(next.id);
  }
  window.addEventListener("keydown", onKeyDown);
  return () => window.removeEventListener("keydown", onKeyDown);
}, [tabs, activeTabId, setActive]);
```

(If Phase 5 already registered a global key handler for hotkey capture, mount this one below it so capture mode takes precedence; they should not conflict because Phase 5 uses physical hotkey APIs via the OS, not DOM `keydown`.)

- [ ] **Step 2: Run the type check + the full test suite**

```powershell
npx tsc --noEmit
npm test
```

Expected: clean type-check; all tests still pass.

- [ ] **Step 3: Commit**

```powershell
git add src/App.tsx
git commit -m "feat(app): Ctrl+Tab / Ctrl+Shift+Tab cycle through tabs"
```

---

## Task 11: Wire `showTimestamps` from store into existing SegmentCard call sites

**Files:**
- Modify: `src/components/TabBody.tsx` (already done in Task 6 — verify)
- Modify: any other component that renders `<SegmentCard />` (search for usages)

The Phase 4 plan has TabBody as the only consumer of SegmentCard. If Phase 5 added a tray preview or a hotkey-mode mini-card that also renders SegmentCard, that call site needs `showTimestamps={…}` too. This task is a defensive sweep.

- [ ] **Step 1: Find all SegmentCard usages**

```powershell
npx tsc --noEmit
# Then:
Select-String -Path "src\**\*.tsx" -Pattern "<SegmentCard" -SimpleMatch
```

- [ ] **Step 2: Pass `showTimestamps` from `useSettingsStore` at each call site**

Mirror the TabBody pattern. If no other call sites exist, this task is a no-op confirmation.

- [ ] **Step 3: Commit (allow empty)**

```powershell
git diff --stat
git commit --allow-empty -m "chore: confirm all SegmentCard usages forward showTimestamps"
```

---

## Task 12: VocabSettings chip editor (OPTIONAL — skip if scope is tight)

**Files:**
- Modify: `src/components/VocabSettings.tsx`
- Modify: `src/__tests__/VocabSettings.test.tsx`
- Modify: `src/styles.css`
- Modify: `src/i18n/locales/pt-BR.json` + `src/i18n/locales/en.json`

> **Decision gate.** This task is explicitly optional per the Phase 6 brief and the Phase 4 spec note. Estimate: ~90 min including tests and styling. If the team's budget for Phase 6 is tight (e.g. spending more on auto-scroll polish), skip this task entirely — Phase 6 acceptance does not require it. Otherwise proceed.

The store API (`vocabTerms: string[]` + `setVocabTerms(terms)`) is unchanged; only the editor view flips.

- [ ] **Step 1: Add i18n keys**

`pt-BR.json` `"settings"`:

```json
    "vocabAddChipHint": "Pressione Enter ou vírgula para adicionar.",
    "vocabRemoveChip": "Remover {{term}}"
```

`en.json` `"settings"`:

```json
    "vocabAddChipHint": "Press Enter or comma to add.",
    "vocabRemoveChip": "Remove {{term}}"
```

- [ ] **Step 2: Rewrite the VocabSettings tests in `src/__tests__/VocabSettings.test.tsx`**

Keep the existing "renders the persisted terms" intent but adapt for chips:

```tsx
it("renders existing terms as chips", () => {
  useSettingsStore.setState({ vocabTerms: ["alpha", "beta"] });
  render(<VocabSettings />);
  expect(screen.getByText("alpha")).toBeInTheDocument();
  expect(screen.getByText("beta")).toBeInTheDocument();
});

it("adds a new chip on Enter and persists", () => {
  render(<VocabSettings />);
  const input = screen.getByRole("textbox");
  fireEvent.change(input, { target: { value: "gamma" } });
  fireEvent.keyDown(input, { key: "Enter" });
  expect(settingsSet).toHaveBeenCalledWith({
    key: "vocab_terms",
    value: JSON.stringify(["gamma"]),
  });
});

it("adds a new chip on comma and persists", () => {
  render(<VocabSettings />);
  const input = screen.getByRole("textbox");
  fireEvent.change(input, { target: { value: "delta," } });
  // Implementations may split on comma in onChange OR onKeyDown; the
  // assertion only requires that the chip ultimately lands.
  expect(settingsSet).toHaveBeenCalledWith({
    key: "vocab_terms",
    value: JSON.stringify(["delta"]),
  });
});

it("removes a chip on its × button", () => {
  useSettingsStore.setState({ vocabTerms: ["alpha", "beta"] });
  render(<VocabSettings />);
  fireEvent.click(screen.getByRole("button", { name: /remove alpha|remover alpha/i }));
  expect(settingsSet).toHaveBeenCalledWith({
    key: "vocab_terms",
    value: JSON.stringify(["beta"]),
  });
});

it("Backspace on empty input removes the last chip", () => {
  useSettingsStore.setState({ vocabTerms: ["alpha", "beta"] });
  render(<VocabSettings />);
  const input = screen.getByRole("textbox");
  fireEvent.keyDown(input, { key: "Backspace" });
  expect(settingsSet).toHaveBeenCalledWith({
    key: "vocab_terms",
    value: JSON.stringify(["alpha"]),
  });
});
```

- [ ] **Step 3: Rewrite `src/components/VocabSettings.tsx`**

```tsx
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { useSettingsStore } from "../stores/settingsStore";

export function VocabSettings() {
  const { t } = useTranslation();
  const vocabTerms = useSettingsStore((s) => s.vocabTerms);
  const setVocabTerms = useSettingsStore((s) => s.setVocabTerms);
  const [draft, setDraft] = useState("");

  function commit(newTerms: string[]) {
    const cleaned = newTerms.map((x) => x.trim()).filter((x) => x.length > 0);
    void setVocabTerms(cleaned);
  }

  function addFromDraft(raw: string) {
    const parts = raw.split(",").map((s) => s.trim()).filter(Boolean);
    if (parts.length === 0) return;
    const next = [...vocabTerms];
    for (const p of parts) if (!next.includes(p)) next.push(p);
    commit(next);
    setDraft("");
  }

  function onKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    if (e.key === "Enter" || e.key === ",") {
      e.preventDefault();
      addFromDraft(draft);
    } else if (e.key === "Backspace" && draft.length === 0 && vocabTerms.length > 0) {
      e.preventDefault();
      commit(vocabTerms.slice(0, -1));
    }
  }

  function onChange(e: React.ChangeEvent<HTMLInputElement>) {
    const v = e.target.value;
    if (v.includes(",")) {
      addFromDraft(v);
    } else {
      setDraft(v);
    }
  }

  function removeChip(term: string) {
    commit(vocabTerms.filter((x) => x !== term));
  }

  return (
    <section className="drawer__section">
      <label htmlFor="vocab-input">{t("settings.vocabHeading")}</label>
      <div className="chip-editor">
        {vocabTerms.map((term) => (
          <span key={term} className="chip">
            {term}
            <button
              className="chip__remove"
              aria-label={t("settings.vocabRemoveChip", { term })}
              onClick={() => removeChip(term)}
            >
              ×
            </button>
          </span>
        ))}
        <input
          id="vocab-input"
          className="chip-editor__input"
          value={draft}
          onChange={onChange}
          onKeyDown={onKeyDown}
          placeholder={t("settings.vocabPlaceholder")}
        />
      </div>
      <small className="drawer__hint">{t("settings.vocabAddChipHint")}</small>
    </section>
  );
}
```

- [ ] **Step 4: Append the chip CSS**

```css
.chip-editor {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  padding: 6px;
  border: 1px solid #3a3a3a;
  background: #1e1e1e;
  min-height: 36px;
}
.chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  background: #2a2a2a;
  color: #fff;
  border-radius: 4px;
  padding: 2px 6px;
  font-size: 12px;
}
.chip__remove {
  background: transparent;
  border: none;
  color: #888;
  cursor: pointer;
  font-size: 14px;
  line-height: 1;
  padding: 0 2px;
}
.chip__remove:hover { color: #fff; }
.chip-editor__input {
  flex: 1;
  min-width: 120px;
  background: transparent;
  color: #fff;
  border: none;
  outline: none;
  font: inherit;
}
```

- [ ] **Step 5: Run tests**

```powershell
npm test -- VocabSettings
```

Expected: 5 new tests pass.

- [ ] **Step 6: Commit**

```powershell
git add src/components/VocabSettings.tsx src/styles.css src/__tests__/VocabSettings.test.tsx src/i18n
git commit -m "feat(VocabSettings): chip editor with add/remove/backspace + comma splitting"
```

---

## Task 13: Full test sweep + type check

A final sanity sweep before manual acceptance.

- [ ] **Step 1: Type check**

```powershell
npx tsc --noEmit
```

Expected: clean.

- [ ] **Step 2: Run all frontend tests**

```powershell
npm test
```

Expected counts:
- Phase 4 baseline: ~28 tests.
- Phase 5 (separate plan, lands first): unknown delta — accept whatever Phase 5 added as the baseline.
- Phase 6 additions: settingsStore (+4), useNearBottom (+3), SegmentCard (+5: 3 timestamp + 2 copy), TabBody (+4), TabStrip (+2), VocabSettings (+5 if Task 12 ran, else 0 net change).
- Expected total Phase 6 delta: ~18–23 new tests.

- [ ] **Step 3: Run the Rust test suite as a sanity check (no Rust changed this phase)**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml
```

Expected: green. Any failure here is unrelated to Phase 6 and must be triaged separately.

- [ ] **Step 4: Commit (allow empty)**

```powershell
git diff --stat
git commit --allow-empty -m "chore: full test sweep passes for Phase 6"
```

---

## Task 14: Manual acceptance pass

This exercises the items unit tests can't fully cover: real WebView2 clipboard, real scroll physics, real overflow.

- [ ] **Step 1: Launch the app**

```powershell
npm run tauri dev
```

- [ ] **Step 2: Timestamps**

1. Open Settings. Confirm "Mostrar timestamps" is ON by default.
2. Existing tabs with segments now show `HH:MM:SS` (24-h) above each segment text.
3. Toggle the checkbox OFF. Confirm timestamps disappear immediately across all cards.
4. Close the app, reopen. Confirm the OFF state was persisted.
5. Toggle back ON and proceed.

- [ ] **Step 3: Copy segment**

1. Hover any card. The copy icon appears next to play/edit.
2. Click it. A green "Copiado!" / "Copied!" appears briefly.
3. Paste into Notepad. The exact `segment.text` is in the buffer (not the timestamp).

- [ ] **Step 4: Copy tab**

1. In the tab body header (top-right area), click "Copiar aba" / "Copy tab".
2. Paste into Notepad. All segments of the active tab appear separated by blank lines.
3. Switch to an empty tab. The button is disabled (greyed out).

- [ ] **Step 5: Cross-segment selection**

1. Click-drag from mid-segment N through into segment N+1.
2. Ctrl+C, paste into Notepad. The selected text spans both segments exactly as seen.
3. If selection visually "breaks" between cards (e.g. each card is a separate flexbox row with `user-select` rules interfering), audit the segment-card CSS for `user-select: none` outside the timestamp.

- [ ] **Step 6: Auto-scroll near bottom**

1. Speak several sentences while staying scrolled to the bottom.
2. **Expected:** the list smooth-scrolls each time a new card lands.

- [ ] **Step 7: Auto-scroll while reading old content (pill behaviour)**

1. Scroll up several screens into older content.
2. Speak a sentence.
3. **Expected:** the list does NOT scroll. A pill labelled "Nova transcrição ↓" / "New transcription ↓" appears near the bottom of the viewport.
4. Click the pill. The list smooth-scrolls to bottom; the pill disappears.
5. Scroll up again, speak two sentences, scroll back down manually (without clicking the pill). Confirm the pill disappears as soon as you reach the near-bottom zone.

- [ ] **Step 8: Empty state**

1. Create a new tab. Switch to it before capture lands any segment.
2. **Expected:** centered hint card with mic glyph + "Comece a falar para transcrever." Visible only after the brief load completes (no flicker of the hint while loading).
3. Speak one sentence. The hint card disappears and the first segment card replaces it.

- [ ] **Step 9: Tab strip overflow**

1. Create tabs until the strip overflows horizontally (~8–10 short titles, fewer with long titles).
2. **Expected:** a subtle dark gradient appears on the right edge. Scroll the strip to the right via wheel/swipe; the right fade disappears and a left fade appears. Scroll back; behaviour mirrors.
3. Click a tab on the far edge. Confirm it scrolls into view comfortably.

- [ ] **Step 10: Segment count badges**

1. With the multi-tab setup, confirm each tab title shows its segment count as a small badge.
2. Create a fresh tab. Confirm its title has NO badge (count is 0).
3. Speak a sentence into a non-active tab (using Phase 5's hotkey + capture mode if applicable, or just confirm the badge updates after switching to that tab and back). The badge should reflect live count.
4. Delete a segment from a tab. The badge decrements without manual refresh.

- [ ] **Step 11: Keyboard nav**

1. With multiple tabs, press Ctrl+Tab. Active tab advances to the next, wrapping past the last.
2. Press Ctrl+Shift+Tab. Goes back, wrapping past the first.
3. Focus a segment's edit textarea. Press Ctrl+Tab inside it. Nothing happens (the listener correctly ignored the input-focused event).

- [ ] **Step 12: Chip editor (only if Task 12 shipped)**

1. Settings → Vocabulário.
2. Existing terms render as chips.
3. Type "newTerm" + Enter → a chip appears. The input clears.
4. Type "a,b,c" → three chips appear simultaneously.
5. Click the × on any chip → it disappears.
6. With an empty input, press Backspace → the last chip is removed.
7. Close and reopen the app. Chips persist.

- [ ] **Step 13: Record results**

If every step passed, Phase 6 acceptance is met. Write a stop-and-report listing pass/fail per step and any UX observations worth feeding into Phase 7+ (e.g. "the pill obscures the bottom segment a little — consider raising it 4 px").

- [ ] **Step 14: Final commit (if any tweaks fell out)**

```powershell
git diff --stat
git commit -am "chore: phase 6 manual acceptance pass"
```

---

## End of Phase 6 — checkpoint

**Stop here and produce a stop-and-report.** Phase 7 (OpenAI cloud STT backend) takes over from here.

**What's verified at this checkpoint:**
- Segment cards show timestamps (toggleable, persisted) and have a one-click copy.
- The full tab can be copied with one click; cross-segment selection works natively.
- The list auto-scrolls only when the user is near the bottom; a pill surfaces missed transcriptions otherwise.
- Empty tabs show a friendly hint, never visible during load.
- The tab strip handles overflow gracefully and surfaces a live segment-count badge per tab.
- Keyboard nav cycles tabs via Ctrl+Tab / Ctrl+Shift+Tab without hijacking input fields.
- (Optional) Vocab editor is a chip component instead of a textarea.

**What's verified that isn't on the brief:**
- `useNearBottom` is a reusable hook (≤ 30 LOC, unit-tested) that any future "infinite scroll" feature can reuse.
- `clipboard.writeText` is the single seam for all copy operations — easy to swap for a Tauri-native clipboard command later if WebView2 ever requires it.
- The empty-state vs. loading-state gating is correct (no flicker, no spinner-during-load to be polished separately).

**Known follow-ups (not blockers):**
- Phase 4's `"emptyState"` i18n key is now unused; remove in a later cleanup once nothing references it.
- The mic glyph in `EmptyTabHint` is an emoji; swap for a proper SVG when a design system lands.
- Pulse-on-receiving-tab animation (spec §8) was not implemented; consider folding into the badge increment as a CSS keyframe in a future pass.
- The gradient fade endpoint colour `#1a1a1a` is hardcoded; if the app introduces a light theme, lift this into a CSS custom property.
