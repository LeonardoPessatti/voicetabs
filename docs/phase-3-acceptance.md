# Phase 3 — manual acceptance

STT subprocess wired into the capture pipeline. Transcribed text appears in logs + as a Tauri event. No segments yet — that's Phase 4.

## Setup

Close any running VoiceTabs instance. Build the release worker once so the bundled externalBin path exists:

```powershell
cargo build --release -p stt_worker --bin stt_worker_cpu
```

(If you have CUDA toolkit and want to also exercise the CUDA path: `cargo build --release --features cuda -p stt_worker --bin stt_worker_cuda`. Without the CUDA toolkit, just the CPU build is enough; the GPU probe will still report CUDA detected on a GTX 1060, but the supervisor will fall back to the CPU binary at launch since the CUDA exe is missing.)

Reset audio + DB so the test starts clean:

```powershell
Remove-Item -Recurse -Force "$env:APPDATA\voicetabs\audio" -ErrorAction SilentlyContinue
npm run tauri dev
```

A window opens.

## Tests

1. **Status dot at startup**
   - Footer shows a small dot between `🎤 Capture: OFF` and `⚙ Settings`.
   - On first launch the dot should show **loading** (gray/yellow) briefly while the supervisor boots and loads the model (~2-5 s), then transition to **ready** (green).
   - Hovering the dot shows the backend + model id (e.g. "STT ready · CUDA · large-v3-turbo-q5_0").

2. **First transcription**
   - Click the capture toggle to start.
   - Speak one short PT-BR sentence (~3 s). Pause ~1 s.
   - Watch the dev console (WebView devtools, right-click → Inspect, then Console tab) for a `stt-transcription` event payload like:
     ```json
     { "request_id": "...", "text": "Olá mundo.", "started_at_ms": ..., "ended_at_ms": ..., "audio_path": "C:\\Users\\...\\audio\\<ts>.wav" }
     ```
   - The WAV file should also exist at `%APPDATA%\voicetabs\audio\<ts>.wav` (same as Phase 2).
   - The `voicetabs.log` file at `%APPDATA%\voicetabs\logs\` should have lines like `INFO stt: transcription request=... text=...`.

3. **No transcription on silence**
   - With capture ON, sit silent for 30 s.
   - No new WAV file. No `stt-transcription` event. Status dot stays **ready**.

4. **GPU autodetect**
   - The first launch should detect the GTX 1060 via `nvidia-smi` and pick `cuda` as the backend (visible in the dot's tooltip + in the `stt_backend` row of the settings table). Subsequent launches read the cached value and don't reprobe.
   - To force re-probe: delete the `stt_backend` row from the SQLite db (or set `WHISPER_CUDA=0` env var to force CPU).

5. **Supervisor restart**
   - With capture ON and the dot showing ready, open Task Manager → find `stt_worker_cpu.exe` (or `stt_worker_cuda.exe`) → End Process.
   - Within ~5 s, the dot should briefly turn yellow (**restarting**) then back to green.
   - Speak another sentence. The transcription should still arrive (replayed once + new utterance).
   - Status dot stays ready after.

6. **Restart**
   - Close the window. Re-run `npm run tauri dev`.
   - Toggle still OFF. Dot still works. Cached `stt_backend` value still in settings (no reprobe).

If any step fails, capture the symptom and stop. If all pass, Phase 3 is done.

## What's NOT in Phase 3

- Transcribed text does NOT appear in any tab. Tab routing + segment writes are **Phase 4**.
- Hallucination filter is **Phase 4** (the `stt-transcription` event today fires for `"obrigado por assistir"` hallucinations too).
- Vocabulary biasing is **Phase 4**.
- First-run wizard is permanently skipped (model is bundled).
