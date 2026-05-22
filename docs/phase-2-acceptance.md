# Phase 2 — manual acceptance

Audio capture + Silero VAD + utterance WAV writing.

## Setup

Close any running VoiceTabs instance. Then in PowerShell:

```powershell
Remove-Item -Recurse -Force "$env:APPDATA\voicetabs\audio" -ErrorAction SilentlyContinue
npm run tauri dev
```

A window opens.

## Tests

1. **No capture by default**
   - Footer shows `🎤 Capturar: OFF` (or `🎤 Capture: OFF` in English).
   - `%APPDATA%\voicetabs\audio\` is empty (or non-existent).

2. **One utterance → one file**
   - Click the toggle. It changes to `🎤 Capturando` in green; OS shows the mic indicator.
   - Speak one short sentence (≈3 s), then pause ~1 s.
   - In `%APPDATA%\voicetabs\audio\` a new `<timestamp>.wav` appears.
   - Open the file in any media player; you should hear your sentence with a brief silence at the start (pre-roll).

3. **Two utterances → two files**
   - Continue capturing. Speak a second short sentence. Pause.
   - A second `<timestamp>.wav` file appears.

4. **60 s of silence → zero new files**
   - Stop speaking. Don't make noise (no typing nearby). Wait 60 s.
   - The file count in the audio directory is unchanged.

5. **Max-cap split**
   - Set a stopwatch. Speak continuously for at least 30 s (read aloud from any text).
   - Two files should appear: the first capped at ~30 s, the second starting where the first left off and ending when you stop.

6. **Toggle off**
   - Click the toggle. It changes back to `🎤 Capturar: OFF`. The OS mic indicator disappears.
   - Speaking should produce no new files until you toggle it back on.

7. **Restart**
   - Close the window. Re-run `npm run tauri dev`.
   - Footer toggle should be OFF again (capture state is not persisted across launches).
   - Files written in this session remain in the audio dir.

If any step fails, capture the symptom and stop. If everything passes, the Phase 2 acceptance is complete.
