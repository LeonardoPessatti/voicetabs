# Phase 4 — manual acceptance

End-to-end: speak → transcribe → segment card under the active tab. This is the first phase where the user sees the result of dictation on screen.

## Setup

Close any running VoiceTabs instance. The release CPU worker needs to exist locally so `npm run tauri dev` doesn't fail the externalBin pre-check:

```powershell
$env:LIBCLANG_PATH = 'C:\Program Files\LLVM\bin'
$env:Path = 'C:\Program Files\CMake\bin;' + $env:Path
cargo build --release -p stt_worker --bin stt_worker_cpu
```

(If you have CUDA toolkit, also build the CUDA variant: `cargo build --release --features cuda -p stt_worker --bin stt_worker_cuda`. Without it, the GPU probe still detects your GTX 1060 but the supervisor falls back to CPU at launch.)

Reset audio + DB so the test starts clean (deletes prior tabs + segments too):

```powershell
Remove-Item -Recurse -Force "$env:APPDATA\voicetabs" -ErrorAction SilentlyContinue
npm run tauri dev
```

A window opens with a fresh "Untitled" tab.

## Tests

### A3 — Tab routing invariant

1. Create three tabs: rename them `Reunião`, `Pessoal`, `Ideias`.
2. Click the capture toggle to start.
3. Click **Reunião** → speak a complete short sentence (~3 s) → pause 1 s.
   - **Expected:** one segment card appears under Reunião with the transcribed text. Pessoal + Ideias stay empty.
4. Click **Pessoal** → speak a different sentence → pause.
   - **Expected:** segment lands under Pessoal. Reunião + Ideias unchanged.
5. **The load-bearing test:** Click **Ideias**. Start speaking. **While still speaking**, click **Reunião**. Finish the sentence. Pause.
   - **Expected:** the complete sentence appears as one card under **Ideias** (the tab that was active when speech began). Reunião gains no new segment.

If step 5 fails (sentence lands under Reunião or splits across both), A3 has regressed.

### A5 (full) — Silence produces nothing

6. With capture still ON: sit silent (no typing nearby) for 60 s.
   - **Expected:** no new segments in any tab. The audio dir gets no new WAVs.

### A6 — Vocab biasing

7. Open `⚙ Configurações` → scroll to **Vocabulário** section.
8. Add these terms (one per line):
   ```
   VoiceTabs
   Tauri
   Whisper
   ```
9. Click outside the textarea to trigger save-on-blur.
10. Speak a sentence that includes "VoiceTabs" or "Tauri": for example, `"Estou testando o VoiceTabs com Tauri e Whisper."`
    - **Expected:** the transcription should include the technical terms spelled correctly (or much closer to correct) compared to without vocab. Try it once before adding vocab, once after, and compare.

### Segment actions

11. On any segment card, hover → click the **▶ Play** button.
    - **Expected:** the original WAV plays via HTML `<audio>`. (You should hear yourself speaking the sentence.)
12. Click the **✎ Edit** button.
    - **Expected:** paragraph swaps to a textarea. Type new text, click **Save**.
    - The segment's `text` updates. The original audio + `original_text` are preserved (visible in DB if you peek).
13. Click the overflow menu (⋮) → **Re-transcribe (current vocab)**.
    - **Expected:** "transcribing…" placeholder briefly, then the segment text updates with the new transcription (uses today's vocab list).
14. Overflow menu → **Re-transcribe (snapshot vocab)**.
    - **Expected:** uses the vocab snapshot stored when the segment was created (not the current vocab). Useful if you change vocab later but want to re-transcribe using the original context.
15. Overflow menu → **Delete**.
    - **Expected:** if the text is ≤20 chars, deletes immediately. If >20 chars, `window.confirm` asks to confirm. After confirming, the segment + its WAV file are both gone.

### Hallucination filter

16. With capture ON, close your microphone (mute mic in Windows audio settings) but leave the capture toggle ON. Speak (you'll see no audio reach the app). After a few seconds:
    - **Expected:** Whisper might still emit `"obrigado por assistir"` or similar internally, but the **hallucination filter drops it before it reaches the segments table**. No card appears. Log file at `%APPDATA%\voicetabs\logs\voicetabs.log` shows `Drop(reason: ...)` lines.

### Restart preservation

17. Close the window. Re-run `npm run tauri dev`.
    - **Expected:** all 3 tabs still there, all segments preserved with their text + audio. Vocab terms still saved. The previously-active tab is still active.

If all 17 steps pass, Phase 4 is done.

## What's NOT in Phase 4

- Push-to-talk hotkey / mode toggle / tray — **Phase 5**
- Polished segment UX (timestamps, copy button, drag-reorder) — **Phase 6**
- Resilience polish (hot-spare worker, recovery within 5 s strict) — **Phase 7**
- Installer + signed binaries — **Phase 8**
