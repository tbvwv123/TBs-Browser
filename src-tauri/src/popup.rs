//! 콘텐츠 위에 뜨는 팝업 (spec 3장 배치, 0단계 5번): 주소창 자동완성, 진행 중인 다운로드 목록.
//! 메인 창이 소유한 투명 창이고, 포커스를 가져가지 않게 띄운다. 메인 창이 움직이면 닫는다.

use std::sync::Mutex;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, EventTarget, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder, Window};
use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWL_EXSTYLE, HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE, WS_EX_NOACTIVATE,
    },
};

use crate::{browser::UI_WEBVIEW, paths};

pub const LABEL: &str = "popup";
/// 그림자를 그릴 여백 (팝업 페이지와 같은 값)
const MARGIN: f64 = 16.0;

/// 팝업을 그릴 테마 ("light" | "dark"). UI가 띄울 때마다 알려 준다
static THEME: Mutex<String> = Mutex::new(String::new());

/// 지금 떠 있는 팝업의 종류 ("suggest" | "downloads" | "menu" | "bookmarks")
static KIND: Mutex<Option<String>> = Mutex::new(None);

pub fn create(app: &AppHandle, main: &Window) -> tauri::Result<()> {
    let w = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html#popup".into()))
        .data_directory(paths::webview_dir())
        .browser_extensions_enabled(crate::dev::extensions())
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        .visible(false)
        .owner_raw(main.hwnd()?)
        .inner_size(10.0, 10.0)
        .build()?;
    if let Ok(h) = w.hwnd() {
        crate::dev::move_to_dev_desktop(h.0 as isize);
        unsafe {
            let ex = GetWindowLongPtrW(HWND(h.0), GWL_EXSTYLE);
            SetWindowLongPtrW(HWND(h.0), GWL_EXSTYLE, ex | WS_EX_NOACTIVATE.0 as isize);
        }
    }
    Ok(())
}

/// 메인 창 안쪽 좌표(논리 px)의 상자 자리에 팝업을 띄운다. 메인 스레드에서 부른다.
pub fn show(app: &AppHandle, kind: &str, x: f64, y: f64, w: f64, h: f64, data: Value) {
    let (Some(popup), Some(main)) = (app.get_webview_window(LABEL), app.get_window(crate::browser::MAIN_WINDOW)) else { return };
    let scale = main.scale_factor().unwrap_or(1.0);
    let Ok(origin) = main.inner_position() else { return };
    let origin = origin.to_logical::<f64>(scale);
    let _ = popup.set_position(LogicalPosition::new(origin.x + x - MARGIN, origin.y + y));
    let _ = popup.set_size(LogicalSize::new(w + MARGIN * 2.0, h + MARGIN));
    if let Some(t) = data.get("theme").and_then(|t| t.as_str()) {
        *THEME.lock().unwrap() = t.to_string();
    }
    let _ = app.emit_to(EventTarget::webview(LABEL), "popup_data", json!({ "kind": kind, "data": data }));
    *KIND.lock().unwrap() = Some(kind.to_string());
    if let Ok(hwnd) = popup.hwnd() {
        unsafe {
            let _ = ShowWindow(HWND(hwnd.0), SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(HWND(hwnd.0), Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
}

pub fn hide(app: &AppHandle) {
    if KIND.lock().unwrap().take().is_none() {
        return;
    }
    if let Some(hwnd) = app.get_webview_window(LABEL).and_then(|p| p.hwnd().ok()) {
        unsafe {
            let _ = ShowWindow(HWND(hwnd.0), SW_HIDE);
        }
    }
    let _ = app.emit_to(EventTarget::webview(UI_WEBVIEW), "popup_closed", ());
}

pub fn is_open() -> bool {
    KIND.lock().unwrap().is_some()
}

pub fn is_showing(kind: &str) -> bool {
    KIND.lock().unwrap().as_deref() == Some(kind)
}

/// 다운로드 목록이 떠 있으면 내용을 새로 보낸다. 진행 중인 것이 없으면 닫는다.
pub fn refresh_downloads(app: &AppHandle) {
    if !is_showing("downloads") {
        return;
    }
    let active: Vec<_> = crate::downloads::list(app).into_iter().filter(|d| d.state == "progress").collect();
    if active.is_empty() {
        hide(app);
        return;
    }
    let _ = app.emit_to(EventTarget::webview(LABEL), "popup_data", json!({ "kind": "downloads", "data": { "items": active, "theme": *THEME.lock().unwrap() } }));
}
