//! 제목 표시줄 없는 창의 틀 (spec 3장 배치, 0단계 6번).
//!
//! - 위쪽 변 크기 조절: Tauri는 `add_child`로 만든 웹뷰에는 크기 조절용 테두리 창을 붙이지 않는다.
//!   웹뷰를 만든 뒤 장식을 켰다 끄면 붙는다.
//! - 스냅 레이아웃: 최대화 버튼 자리에 작은 자식 창을 얹고 `WM_NCHITTEST`에 `HTMAXBUTTON`을 돌려준다.
//!   이 창이 마우스를 가져가므로, 마우스 올림 표시는 호스트가 UI에 이벤트로 알린다.

use std::sync::{
    atomic::{AtomicBool, AtomicIsize, Ordering},
    OnceLock,
};

use tauri::{AppHandle, Emitter, EventTarget, Window};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT},
            WindowsAndMessaging::*,
        },
    },
};

use crate::browser::UI_WEBVIEW;

static APP: OnceLock<AppHandle> = OnceLock::new();
static OVERLAY: AtomicIsize = AtomicIsize::new(0);
static HOVER: AtomicBool = AtomicBool::new(false);

/// 웹뷰를 다 얹은 뒤, 창을 보이기 전에 부른다.
pub fn install(app: &AppHandle, window: &Window) {
    let _ = APP.set(app.clone());
    // 위쪽 변 크기 조절용 테두리 창을 붙인다
    let _ = window.set_decorations(true);
    let _ = window.set_decorations(false);
    let Ok(parent) = window.hwnd() else { return };
    unsafe {
        let class = w!("TBS_SNAP_OVERLAY");
        let hinstance = HINSTANCE(GetModuleHandleW(PCWSTR::null()).unwrap_or_default().0);
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(overlay_proc),
            hInstance: hinstance,
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&wc);
        // 레이어드 자식 창은 만들어지지 않는다 (0단계 6번). 평범한 자식 창을 쓴다.
        if let Ok(h) = CreateWindowExW(
            WS_EX_NOACTIVATE,
            class,
            PCWSTR::null(),
            WS_CHILD | WS_CLIPSIBLINGS,
            0,
            0,
            0,
            0,
            Some(HWND(parent.0)),
            None,
            Some(hinstance),
            None,
        ) {
            OVERLAY.store(h.0 as isize, Ordering::SeqCst);
        }
    }
}

/// UI가 알려 준 최대화 버튼 자리(논리 좌표)에 덮개 창을 놓는다.
pub fn place_max_button(window: &Window, x: f64, y: f64, w: f64, h: f64) {
    let hwnd = OVERLAY.load(Ordering::SeqCst);
    if hwnd == 0 {
        return;
    }
    let s = window.scale_factor().unwrap_or(1.0);
    let (px, py, pw, ph) = ((x * s).round() as i32, (y * s).round() as i32, (w * s).round() as i32, (h * s).round() as i32);
    unsafe {
        let _ = SetWindowPos(HWND(hwnd as _), Some(HWND_TOP), px, py, pw, ph, SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
}

fn emit_hover(hover: bool) {
    if HOVER.swap(hover, Ordering::SeqCst) != hover {
        if let Some(app) = APP.get() {
            let _ = app.emit_to(EventTarget::webview(UI_WEBVIEW), "max_button_hover", hover);
        }
    }
}

unsafe extern "system" fn overlay_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_NCHITTEST => LRESULT(HTMAXBUTTON as isize),
            WM_NCMOUSEMOVE => {
                if !HOVER.load(Ordering::SeqCst) {
                    let mut t = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE | TME_NONCLIENT,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    let _ = TrackMouseEvent(&mut t);
                }
                emit_hover(true);
                LRESULT(0)
            }
            WM_NCMOUSELEAVE => {
                emit_hover(false);
                LRESULT(0)
            }
            // 누를 때는 아무것도 하지 않고, 뗄 때 최대화·복원한다 (Windows 버튼과 같다)
            WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK => LRESULT(0),
            WM_NCLBUTTONUP => {
                if let Ok(parent) = GetParent(hwnd) {
                    let cmd = if IsZoomed(parent).as_bool() { SW_RESTORE } else { SW_MAXIMIZE };
                    let _ = ShowWindow(parent, cmd);
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
