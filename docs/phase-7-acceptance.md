# Phase 7 — manual acceptance

Hybrid local-CPU / OpenAI transcription backend selectable at runtime from the
settings drawer. Both backends sit behind the `SttBackend` trait; the drainer
and `segments_retranscribe` call the trait, not the supervisor directly. A
user can flip "Local (CPU)" ↔ "OpenAI" mid-session and the very next utterance
is routed through the chosen path.

This recipe drives every acceptance criterion (A-P7-1 through A-P7-8) from
`docs/superpowers/plans/2026-05-23-voicetabs-phase-7-openai-backend.md`.

## Setup

```powershell
git pull
$env:LIBCLANG_PATH = 'C:\Program Files\LLVM\bin'
$env:Path          = 'C:\Program Files\CMake\bin;' + $env:Path

# Release worker — Phase 7 still routes the Local backend through the
# subprocess supervisor; the dev shell expects an already-built worker.
cargo build --release -p stt_worker --bin stt_worker_cpu

npm install
npm run tauri dev
```

A window opens with whatever tabs / segments you had at the end of Phase 6.

You will need:
- A microphone.
- An OpenAI API key (used **only** for steps A-P7-3 / 4). Cost per
  test utterance is negligible (`gpt-4o-mini-transcribe` is ~$0.003 / minute
  of audio).
- `sqlite3.exe` on `PATH` (used by A-P7-2 / 4 to inspect `voicetabs.db`).
- `cmdkey.exe` (ships with Windows; verifies Credential Manager entries).
- A second invalid key string such as `sk-invalid-test-key-xxxx` (used for
  A-P7-5).

Locations referenced below:
- DB:   `$env:APPDATA\voicetabs\voicetabs.db`
- Logs: `$env:APPDATA\voicetabs\logs\voicetabs.log`
- Credential Manager service: `voicetabs` / key `openai_api_key`

---

## A-P7-1 — default offline path still works

Fresh install (no keyring entry, no `stt_backend` setting).

1. With a clean `%APPDATA%\voicetabs\voicetabs.db` (or a pre-existing one
   where you have **never** opened the backend settings), confirm
   `cmdkey /list | Select-String voicetabs` prints nothing.
2. Launch the app (`npm run tauri dev`).
3. The status dot at the top is green; hovering shows
   `(Local CPU · ggml-small-q5_1)` (or whatever local whisper model is
   bundled — the exact suffix is the file name minus `.bin`).
4. Press the capture toggle (or your bound hotkey from Phase 5) and speak a
   short PT-BR sentence.
5. **Expected:** within ~1–1.5 s a segment card appears under the active tab
   with the transcribed text. No errors in the log; no network activity
   (verify with `Get-NetTCPConnection -State Established | Where-Object
   { $_.OwningProcess -eq (Get-Process voicetabs).Id }` — should not include
   `api.openai.com` resolved IPs).

This proves no regression on Phase 3 / 4 / 5 / 6.

---

## A-P7-2 — saving the API key writes only to the keyring

6. Open the settings drawer (`⚙ Configurações`). Scroll to the new
   **"Backend de transcrição"** section.
7. Click the **OpenAI (cloud)** radio. The key input + Save / Clear buttons
   appear, plus the disclaimer
   `"Áudio enviado para api.openai.com. Veja a política de privacidade da OpenAI."`.
8. Paste a real key into the field. Click **Salvar chave**.
9. The status line under the buttons flips to `"Chave configurada ✓"`.
10. Verify Credential Manager:
    ```powershell
    cmdkey /list | Select-String voicetabs
    # Expect a line like:
    # Target: voicetabs
    # Type: Generic
    ```
11. Verify SQLite holds **only the bool**, never the key value:
    ```powershell
    $db = "$env:APPDATA\voicetabs\voicetabs.db"
    sqlite3 $db "select key, value from settings where key in ('stt_backend','openai_api_key_set','openai_api_key');"
    # Expect exactly two rows:
    #   openai_api_key_set|true
    #   stt_backend|<whatever you have selected — may still be "local" here>
    # The 'openai_api_key' row MUST NOT exist.
    ```
12. Verify the log file contains **no** trace of the key. Pick the last 4
    characters of the key you saved (call them `LAST4`) and search:
    ```powershell
    $log = "$env:APPDATA\voicetabs\logs\voicetabs.log"
    Get-Content $log | Select-String -Pattern "sk-" -CaseSensitive:$false
    Get-Content $log | Select-String -Pattern $LAST4
    # Both expected to print nothing.
    ```

---

## A-P7-3 — cloud happy path

13. Drawer still open, **OpenAI** radio still selected, key configured from
    step 9.
14. Close the drawer.
15. Hover the status dot — title now reads
    `(OpenAI · gpt-4o-mini-transcribe)`.
16. Speak one PT-BR sentence (e.g. `"Olá, isso é um teste do backend
    OpenAI."`).
17. **Expected:** a segment card appears under the active tab carrying the
    transcribed text. The WAV file for this utterance still lives in
    `%APPDATA%\voicetabs\audio\` (the cloud backend uploads in-memory but
    the capture controller saves locally regardless).
18. Verify the segment's stored backend / model via:
    ```powershell
    sqlite3 $db "select backend, model_id, substr(text, 1, 40) from segments order by created_at_ms desc limit 1;"
    # Expect:  openai|gpt-4o-mini-transcribe|Olá, isso é um teste do backend OpenAI.
    ```

---

## A-P7-4 — mid-session swap (Local → OpenAI → Local)

This is the most important behavioural test of Phase 7.

19. Open the drawer. Switch back to **Local (CPU)**. Close the drawer.
20. Hover status dot → title back to `(Local CPU · ggml-small-q5_1)`.
21. Speak: `"Primeira frase no backend local."`. Wait for the segment.
22. Open the drawer. Switch to **OpenAI**. Close the drawer.
23. Speak: `"Segunda frase pelo backend OpenAI."`. Wait for the segment.
24. Open the drawer. Switch to **Local (CPU)**. Close the drawer.
25. Speak: `"Terceira frase de volta no backend local."`. Wait for the
    segment.
26. **Expected:** all three cards appear in the same tab, in chronological
    order, no duplicates, no losses.
27. Confirm each row's backend / model_id:
    ```powershell
    sqlite3 $db "select backend, model_id, substr(text, 1, 50) from segments order by created_at_ms desc limit 3;"
    # Expect (newest first):
    #   cpu   |ggml-small-q5_1            |Terceira frase de volta no backend local.
    #   openai|gpt-4o-mini-transcribe     |Segunda frase pelo backend OpenAI.
    #   cpu   |ggml-small-q5_1            |Primeira frase no backend local.
    ```

The hot-swap is correct iff: (a) every row's `backend` matches whatever was
selected at the moment the utterance ENDED, and (b) no row was dropped during
the swap.

---

## A-P7-5 — invalid key produces a red dot + masked log line

28. Open the drawer. Click **Limpar chave** to remove the real key.
29. Paste `sk-invalid-test-key-xxxx` into the field. Click **Salvar chave**.
    Status reads `"Chave configurada ✓"` — the app does not pre-validate.
30. Switch the radio to **OpenAI**. Close the drawer.
31. Speak: `"Esse áudio deve falhar."`.
32. **Expected:**
    - No segment card appears.
    - The status dot turns **red**. Its title shows
      `"OpenAI: chave inválida"`.
33. Verify the log line masks the key:
    ```powershell
    $log = "$env:APPDATA\voicetabs\logs\voicetabs.log"
    Get-Content $log | Select-String -Pattern "OpenAI" | Select-Object -Last 5
    # Expect a line containing the masked form 'sk-i**********' or similar.
    # MUST NOT contain the trailing 'xxxx' (last 4 chars of the test key).
    Get-Content $log | Select-String -Pattern "xxxx"
    # Expect: no output.
    ```

---

## A-P7-6 — clearing the key auto-reverts the backend to Local

34. With OpenAI still selected from step 30 (and the invalid key still in the
    keyring), open the drawer.
35. Click **Limpar chave**.
36. **Expected (all in the same UI tick):**
    - The status line under the buttons flips to
      `"Nenhuma chave configurada"`.
    - The radio resets to **Local (CPU)** automatically (see
      `openai_key_clear` in `src-tauri/src/commands/backend.rs` — it falls
      back to local when OpenAI was active).
    - The status dot returns to **green Local**.
37. Verify Credential Manager:
    ```powershell
    cmdkey /list | Select-String voicetabs
    # Expect: no output.
    ```
38. Verify SQLite:
    ```powershell
    sqlite3 $db "select key, value from settings where key in ('stt_backend','openai_api_key_set');"
    # Expect:
    #   openai_api_key_set|false
    #   stt_backend|local
    ```

---

## Key persistence across restarts (extra check tied to A-P7-2)

39. Save a real key again via the drawer.
40. Close the app via tray → Sair (so the process fully exits — closing the
    window only hides to tray since Phase 5).
41. Verify the keyring entry survives the exit:
    ```powershell
    cmdkey /list | Select-String voicetabs
    # Expect: one Target: voicetabs row.
    ```
42. Reopen the app (`npm run tauri dev`). Open the drawer → Backend section.
43. **Expected:** the **OpenAI** radio is selected (because `stt_backend`
    persisted as `openai` from before exit), and the status line reads
    `"Chave configurada ✓"` — meaning the boot-time read from the keyring
    succeeded.
44. Speak a sentence. **Expected:** segment card lands, routed through
    OpenAI. This proves the key is durable across full process restarts and
    is fetched from the credential store at backend-construction time, not
    cached in process memory only.

---

## A-P7-7 — automated test suites

```powershell
$env:LIBCLANG_PATH = 'C:\Program Files\LLVM\bin'
$env:Path          = 'C:\Program Files\CMake\bin;' + $env:Path

cargo test --workspace
# Expect: 125 passed; 0 failed (4 ignored: hardware-gated PTT / audio
# device tests + their stt_worker counterparts).

npm test
# Expect: 13 test files / 66 tests passed.

npx tsc --noEmit
# Expect: no output (success).

cargo clippy --workspace --all-targets -- -D warnings
# Expect: 'Finished `dev` profile' — no warnings, no errors.
```

These four green lines are the minimum bar before pushing.

---

## A-P7-8 — CI

45. Push the branch. Open the GitHub Actions run for the commit on `master`.
46. **Expected:** every job green. The release-worker prebuild step (added
    in commit `ff2d7cf`) and the bundle-resources Whisper-model stub (added
    in `404fdbc`) should both succeed without intervention.

---

## Record results

If every step passed:
- Phase 7 acceptance is met.
- The hybrid local/cloud backend ships per spec.
- The original "fully offline" property still holds when the user never
  touches the backend section.

Note any UX observations worth feeding into a future polish pass (e.g.
"a save-and-test button would catch invalid keys before the first
utterance fails", "the radio could show a tiny cost-per-minute hint
under OpenAI"). None of those are blockers for v1.
