# Phase 5 — manual acceptance

Global hotkey (keyboard + mouse) + system tray + capture modes (always-on / push-to-talk).

## Setup

```powershell
git pull
$env:LIBCLANG_PATH = 'C:\Program Files\LLVM\bin'
$env:Path = 'C:\Program Files\CMake\bin;' + $env:Path
cargo build --release -p stt_worker --bin stt_worker_cpu
npm run tauri dev
```

A window opens. Settings → drawer has new section **Captura** with mode dropdown and hotkey binding row.

## Tests

### Tray icon

1. Tray icon appears in the system tray (bottom-right). Idle = gray dot.
2. Left-click the icon → window toggles visibility.
3. Right-click → menu shows: Mostrar/Ocultar, Captura: on/off, Modo: always-on/PTT, Sair.
4. Click "Modo" item → toggles between always-on and PTT (visible in settings on next open).
5. Click the X (close) button on the window → window hides to tray (does NOT exit).
6. Click "Sair" in tray menu → app exits cleanly.

### Always-on mode (existing behavior, regression check)

7. Re-launch. Settings → Modo de captura: Sempre ativo. Click capture toggle ON.
8. Speak → segment card appears within ~1-3s (CPU+small). Same as Phase 4.

### Push-to-talk hotkey binding

9. Settings → Captura → click **Vincular**. Status changes to "Pressione qualquer tecla ou botão" (yellow/highlight).
10. Press **F13** (or hold Right Ctrl). The binding row updates to show the captured key.
11. Cancel mid-capture: click "Cancelar" or press Esc → status returns to no-bind/previous bind.

### Push-to-talk hotkey binding — mouse button

12. Repeat step 9. This time press **mouse button 4 or 5** (side buttons). Bind row should show "Mouse Button 4" or similar.
13. If you don't have side mouse buttons, skip this step.

### PTT mode acceptance (A4 — load-bearing)

14. Set Modo = Push-to-talk. Hotkey bound from step 10 or 12.
15. Click capture toggle ON. (In PTT mode, capture is "armed" but no audio gets through until you press the hotkey.)
16. **Hold** the hotkey. Speak a sentence. **Release** the hotkey.
17. Within ~2s, a segment card appears in the active tab.
18. **Minimize the window** (or focus another app).
19. Hold hotkey, speak, release. **Expected**: segment still lands. A4 PASSES.
20. Verify the OS mic indicator is OFF when hotkey is NOT held in PTT mode (or at least pulses with each press).

### Mode hot-reload

21. With capture ON, switch Modo from PTT to Sempre ativo (in settings or tray menu). 
22. Speak without holding any hotkey → segment lands (VAD-driven).
23. Switch back to PTT → speak without hotkey → no segment. Hold hotkey → segment.

### Cleanup

24. Close + relaunch. Mode + hotkey persist.

If all pass, Phase 5 acceptance is complete.

## What's NOT in Phase 5

- Tray menu labels don't re-render on locale change (baked at startup)
- Hotkey capture only listens to a small set of common PTT keys when typing on the keyboard side (F13-F19, modifiers, Space, etc.) — mouse buttons cover the rest
- Cloud STT (Phase 7)
- Segment polish/timestamps/copy buttons (Phase 6)
