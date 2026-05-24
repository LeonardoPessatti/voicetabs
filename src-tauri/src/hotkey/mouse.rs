//! Mouse-button hotkey backend.
//!
//! Owns a dedicated OS thread that installs `WH_MOUSE_LL`, pumps the message
//! loop, and forwards `XBUTTONDOWN` / `XBUTTONUP` for XBUTTON1 (MB4) and
//! XBUTTON2 (MB5). The hook is read-only; we always call `CallNextHookEx`.
//!
//! Other mouse buttons (left/right/middle, wheel) are filtered out — binding
//! the left mouse button would brick the UI, and the v1 spec only requires
//! MB4/MB5 anyway.

use std::sync::OnceLock;

use crossbeam_channel::Sender;
use parking_lot::Mutex;

use super::{Binding, BindingError, HotkeyEvent};

/// What the hook callback emits. Press/release for whichever XBUTTON.
#[derive(Debug, Clone, Copy)]
enum MouseHookEvent {
    XButtonDown(u16), // 1 or 2
    XButtonUp(u16),
}

/// Global channel used by the hook callback. `OnceLock` is sufficient because
/// the hook thread is a singleton for the process lifetime.
static HOOK_TX: OnceLock<Sender<MouseHookEvent>> = OnceLock::new();

pub struct MouseBackend {
    #[allow(dead_code)]
    event_tx: Sender<HotkeyEvent>,
    bound_button: std::sync::Arc<Mutex<Option<u16>>>,
    capture_tx: std::sync::Arc<Mutex<Option<Sender<Result<Binding, BindingError>>>>>,
}

impl MouseBackend {
    pub fn new(event_tx: Sender<HotkeyEvent>) -> anyhow::Result<Self> {
        // Spawn the singleton hook thread the first time we're constructed.
        let (tx, rx) = crossbeam_channel::unbounded();
        HOOK_TX
            .set(tx)
            .map_err(|_| anyhow::anyhow!("MouseBackend can only be constructed once"))?;
        std::thread::Builder::new()
            .name("voicetabs-mouse-hook".into())
            .spawn(install_and_pump)
            .map_err(|e| anyhow::anyhow!("spawn mouse hook thread: {e}"))?;

        let bound_button = std::sync::Arc::new(Mutex::new(None::<u16>));
        let capture_tx = std::sync::Arc::new(Mutex::new(
            None::<Sender<Result<Binding, BindingError>>>,
        ));

        // Spawn a forwarder that consumes raw MouseHookEvent and dispatches:
        // - if a binding matches the button: emit HotkeyEvent::Press/Release
        // - if capture is armed: emit Binding into capture_tx
        {
            let event_tx_inner = event_tx.clone();
            let bound_button_inner = bound_button.clone();
            let capture_tx_inner = capture_tx.clone();
            std::thread::Builder::new()
                .name("voicetabs-mouse-dispatch".into())
                .spawn(move || {
                    while let Ok(evt) = rx.recv() {
                        // Capture mode wins over watch mode: a fresh
                        // start_capture should not also fire HotkeyEvent.
                        let captured = match evt {
                            MouseHookEvent::XButtonDown(n) => {
                                if let Some(sender) = capture_tx_inner.lock().take() {
                                    let code = format!("MouseButton{}", n + 3);
                                    let _ = sender.send(Ok(Binding::mouse(code)));
                                    true
                                } else {
                                    false
                                }
                            }
                            MouseHookEvent::XButtonUp(_) => false,
                        };
                        if captured {
                            continue;
                        }
                        let bound = *bound_button_inner.lock();
                        let matches = bound.map(|b| match evt {
                            MouseHookEvent::XButtonDown(n) => b == n,
                            MouseHookEvent::XButtonUp(n) => b == n,
                        });
                        if !matches.unwrap_or(false) {
                            continue;
                        }
                        match evt {
                            MouseHookEvent::XButtonDown(_) => {
                                let _ = event_tx_inner.send(HotkeyEvent::Press);
                            }
                            MouseHookEvent::XButtonUp(_) => {
                                let _ = event_tx_inner.send(HotkeyEvent::Release);
                            }
                        }
                    }
                })
                .map_err(|e| anyhow::anyhow!("spawn mouse dispatch thread: {e}"))?;
        }

        Ok(Self {
            event_tx,
            bound_button,
            capture_tx,
        })
    }

    pub fn watch(&self, code: &str) -> anyhow::Result<()> {
        let n = code_to_xbutton(code).map_err(|e| anyhow::anyhow!(e))?;
        *self.bound_button.lock() = Some(n);
        Ok(())
    }

    pub fn unwatch(&self) {
        *self.bound_button.lock() = None;
    }

    pub fn start_capture(
        &self,
        tx: Sender<Result<Binding, BindingError>>,
    ) -> anyhow::Result<()> {
        *self.capture_tx.lock() = Some(tx);
        Ok(())
    }

    pub fn stop_capture(&self) {
        *self.capture_tx.lock() = None;
    }
}

fn code_to_xbutton(code: &str) -> Result<u16, BindingError> {
    match code {
        "MouseButton4" => Ok(1),
        "MouseButton5" => Ok(2),
        other => Err(BindingError::UnsupportedMouseButton(other.into())),
    }
}

// --- Win32 hook thread + callback --------------------------------------------

#[cfg(target_os = "windows")]
static HOOK_HANDLE: std::sync::atomic::AtomicPtr<core::ffi::c_void> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

#[cfg(target_os = "windows")]
unsafe extern "system" fn hook_proc(
    code: i32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, HHOOK, MSLLHOOKSTRUCT, WM_XBUTTONDOWN, WM_XBUTTONUP,
    };
    if code >= 0 {
        let wm = wparam.0 as u32;
        if wm == WM_XBUTTONDOWN || wm == WM_XBUTTONUP {
            // High word of mouseData identifies XBUTTON1 (=1) or XBUTTON2 (=2).
            let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
            let xbutton = ((info.mouseData >> 16) & 0xFFFF) as u16;
            if xbutton == 1 || xbutton == 2 {
                if let Some(tx) = HOOK_TX.get() {
                    let evt = if wm == WM_XBUTTONDOWN {
                        MouseHookEvent::XButtonDown(xbutton)
                    } else {
                        MouseHookEvent::XButtonUp(xbutton)
                    };
                    let _ = tx.send(evt);
                }
            }
        }
    }
    // ALWAYS chain. We never consume mouse input.
    let ptr = HOOK_HANDLE.load(std::sync::atomic::Ordering::Relaxed);
    let hhook = if ptr.is_null() {
        HHOOK::default()
    } else {
        HHOOK(ptr)
    };
    CallNextHookEx(hhook, code, wparam, lparam)
}

#[cfg(target_os = "windows")]
fn install_and_pump() {
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage, MSG, WH_MOUSE_LL,
    };

    unsafe {
        let hmod = GetModuleHandleW(None).expect("GetModuleHandleW");
        let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(hook_proc), hmod, 0)
            .expect("SetWindowsHookExW(WH_MOUSE_LL)");
        HOOK_HANDLE.store(hook.0, std::sync::atomic::Ordering::Relaxed);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn install_and_pump() {
    // Non-Windows is out of scope for v1; the thread just parks.
    loop {
        std::thread::park();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_to_xbutton_accepts_mb4_mb5() {
        assert_eq!(code_to_xbutton("MouseButton4").unwrap(), 1);
        assert_eq!(code_to_xbutton("MouseButton5").unwrap(), 2);
    }

    #[test]
    fn code_to_xbutton_rejects_left_button() {
        assert!(matches!(
            code_to_xbutton("MouseButtonLeft"),
            Err(BindingError::UnsupportedMouseButton(_))
        ));
    }
}
