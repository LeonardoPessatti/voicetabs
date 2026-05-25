# Phase 6 — manual acceptance

Segment polish + UX: timestamps, copy buttons, intelligent auto-scroll, empty-state hint, tab strip overflow + badges, keyboard nav, vocab chip editor.

## Setup

```powershell
git pull
$env:LIBCLANG_PATH = 'C:\Program Files\LLVM\bin'
$env:Path = 'C:\Program Files\CMake\bin;' + $env:Path
cargo build --release -p stt_worker --bin stt_worker_cpu
npm run tauri dev
```

A window opens with whatever tabs you had at the end of Phase 5. (No need to wipe `%APPDATA%\voicetabs` — Phase 6 is purely UI; pre-existing tabs/segments make a better test bed.)

## Tests

### Timestamps

1. Open Settings (`⚙ Configurações`). Confirm "Mostrar timestamps" / "Show timestamps" is ON by default.
2. Existing tabs with segments now show `HH:MM:SS` (24-h, local time) above each segment text.
3. Toggle the checkbox OFF. Confirm timestamps disappear immediately across all cards in all tabs.
4. Close the app (tray → Sair), reopen. Confirm the OFF state was persisted.
5. Toggle back ON before continuing.

### Copy segment

6. Hover any segment card. A copy icon appears next to the play/edit/menu buttons.
7. Click it. A green "Copiado!" / "Copied!" badge appears briefly and fades.
8. Paste into Notepad. The exact `segment.text` is in the buffer (no timestamp prefix, no extra whitespace).

### Copy tab

9. In the tab body header (top-right area), click **Copiar aba** / **Copy tab**.
10. Paste into Notepad. All segments of the active tab appear separated by blank lines, in order.
11. Switch to an empty tab. The "Copy tab" button is disabled (greyed out, not clickable).

### Cross-segment selection

12. Click-drag from mid-segment N through into segment N+1.
13. Ctrl+C, paste into Notepad. The selected text spans both segments exactly as visually highlighted.
14. The timestamp text itself should NOT be selectable (it has `user-select: none`); selection should jump cleanly from segment text to segment text.

### Auto-scroll near bottom

15. With capture ON in always-on mode, ensure you're scrolled to the bottom of an active tab.
16. Speak several sentences. **Expected:** the list smooth-scrolls each time a new card lands.

### Auto-scroll while reading old content (pill behaviour)

17. Scroll up several screens into older content (or scroll up in any tab with ≥5 segments).
18. Speak a sentence (or wait for one to arrive — Phase 5 PTT also works here).
19. **Expected:** the list does NOT scroll. A pill labelled "Nova transcrição ↓" / "New transcription ↓" appears near the bottom of the viewport.
20. Click the pill. The list smooth-scrolls to bottom; the pill disappears.
21. Scroll up again, speak two sentences, scroll back down manually (without clicking the pill). Confirm the pill disappears as soon as you reach the near-bottom zone (~64 px from bottom).

### Empty state

22. Create a new tab via the `+` button. Switch to it before capture lands any segment.
23. **Expected:** a centered hint card with a mic glyph + "Comece a falar para transcrever." / "Start speaking to transcribe."
24. The hint is visible only after the brief load completes (no flicker of the hint while the segments are being fetched).
25. Speak one sentence. The hint card disappears and the first segment card replaces it.

### Tab strip overflow

26. Create tabs until the strip overflows horizontally (~8–10 short titles; fewer with long titles).
27. **Expected:** a subtle dark gradient appears on the right edge of the strip.
28. Scroll the strip to the right via wheel or swipe. The right fade disappears and a left fade appears. Scroll back; behaviour mirrors.
29. Click a tab on the far edge. Confirm it scrolls into view comfortably (no clipped tab, no jump).

### Segment count badges

30. With the multi-tab setup from step 26, confirm each tab title shows its segment count as a small numeric badge.
31. Create a fresh empty tab. Confirm its title has NO badge (count = 0 → badge hidden).
32. Speak a sentence into the active tab. The badge increments without manual refresh.
33. Delete a segment from a tab via the overflow menu → Excluir. The badge decrements live.

### Keyboard nav

34. With multiple tabs, press **Ctrl+Tab**. Active tab advances to the next, wrapping past the last back to the first.
35. Press **Ctrl+Shift+Tab**. Goes back, wrapping past the first to the last.
36. Click on a segment's edit textarea so it has focus. Press Ctrl+Tab inside it. **Expected:** nothing happens — the listener correctly ignores events fired from input/textarea elements, so it never steals from native focus traversal.
37. Click on the vocab input chip editor field. Press Ctrl+Tab. Same: no tab cycle.

### Vocab chip editor

38. Settings → Vocabulário section.
39. Existing terms (if any) render as chips with an `×` next to each.
40. Type "newTerm" + Enter → a chip appears. The input clears.
41. Type "a,b,c" → three chips appear simultaneously (comma triggers split).
42. Click the `×` on any chip → it disappears immediately.
43. With an empty input, press Backspace → the last chip is removed.
44. Close the app, reopen. Chips persist exactly as left.
45. Open settings again. Speak a sentence using one of the new vocab terms. The transcription should bias toward the correct spelling (smoke-test, not strict).

### Record results

If every step passed, Phase 6 acceptance is met. Note any UX observations worth feeding into a future polish pass (e.g. "the pill obscures the bottom segment a little — consider raising it 4 px", "badge contrast looks weak on dark grey tab background").

## What's NOT in Phase 6

- Hot-spare worker / sub-5s recovery — **Phase 7+**
- Drag-reorder segments within a tab — **future**
- Installer + signed binaries — **Phase 8**
- Pulse-on-receiving-tab animation (spec §8) — folded into a future badge keyframe pass

## Known follow-ups (not blockers for Phase 6)

- Phase 4's `"emptyState"` i18n key is now unused; remove in a later cleanup once nothing references it.
- The mic glyph in `EmptyTabHint` is an emoji; swap for a proper SVG when a design system lands.
- The gradient fade endpoint colour `#1a1a1a` is hardcoded; lift to a CSS custom property if a light theme arrives.
