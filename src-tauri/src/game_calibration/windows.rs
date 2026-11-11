use super::{publish_view, run_hotkey, Hotkey, Hotkeys};
use crate::{text, AppState};
use tauri::{AppHandle, Manager, WebviewWindow};
use uuid::Uuid;
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    UI::{
        Input::KeyboardAndMouse::{
            RegisterHotKey, UnregisterHotKey, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, VK_F8, VK_F9,
        },
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{IsWindow, MA_NOACTIVATE, WM_HOTKEY, WM_MOUSEACTIVATE, WM_NCDESTROY},
    },
};

const SUBCLASS_ID: usize = 1;
const MEASURE_ID: i32 = 0x4d41;
const APPLY_ID: i32 = 0x4d42;

struct Context {
    app: AppHandle,
    session: Uuid,
}

pub(super) fn install(window: &WebviewWindow, session: Uuid) -> Result<(), String> {
    let handle = window.hwnd().map_err(text)?.0 as usize;
    let app = window.app_handle().clone();
    window
        .run_on_main_thread(move || {
            let Some(state) = app.try_state::<AppState>() else {
                return;
            };
            if !state.game_calibration.current.lock().is_ok_and(|current| {
                current
                    .as_ref()
                    .is_some_and(|current| current.id == session)
            }) {
                return;
            }
            let hwnd = handle as HWND;
            let context = Box::into_raw(Box::new(Context {
                app: app.clone(),
                session,
            }));
            let hotkeys = unsafe {
                // A subclass must be installed on the thread that owns this window.
                if IsWindow(hwnd) == 0
                    || SetWindowSubclass(hwnd, Some(window_proc), SUBCLASS_ID, context as usize)
                        == 0
                {
                    drop(Box::from_raw(context));
                    state
                        .client
                        .log("Could not install calibration window input handler".into());
                    return;
                }
                let modifiers = MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT;
                Hotkeys {
                    measure: RegisterHotKey(hwnd, MEASURE_ID, modifiers, u32::from(VK_F8)) != 0,
                    apply: RegisterHotKey(hwnd, APPLY_ID, modifiers, u32::from(VK_F9)) != 0,
                }
            };
            if let Ok(mut current) = state.game_calibration.current.lock() {
                if let Some(current) = current.as_mut().filter(|current| current.id == session) {
                    current.hotkeys = hotkeys;
                }
            }
            publish_view(&app, &state);
        })
        .map_err(text)
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    subclass: usize,
    data: usize,
) -> LRESULT {
    if message == WM_MOUSEACTIVATE {
        // Keep the game in front and deliver the click to the overlay control.
        return MA_NOACTIVATE as LRESULT;
    }
    if message == WM_HOTKEY {
        let key = match wparam as i32 {
            MEASURE_ID => Some(Hotkey::Measure),
            APPLY_ID => Some(Hotkey::Apply),
            _ => None,
        };
        if let Some(key) = key {
            let context = &*(data as *const Context);
            let app = context.app.clone();
            let session = context.session;
            tauri::async_runtime::spawn_blocking(move || {
                if let Err(error) = run_hotkey(&app, session, key) {
                    if let Some(state) = app.try_state::<AppState>() {
                        state.client.log(error);
                    }
                }
            });
            return 0;
        }
    }
    if message == WM_NCDESTROY {
        UnregisterHotKey(hwnd, MEASURE_ID);
        UnregisterHotKey(hwnd, APPLY_ID);
        if RemoveWindowSubclass(hwnd, Some(window_proc), subclass) != 0 {
            drop(Box::from_raw(data as *mut Context));
        }
    }
    DefSubclassProc(hwnd, message, wparam, lparam)
}
