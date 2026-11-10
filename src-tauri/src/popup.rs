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

/// 지금 떠 있는 팝업의 자리 (메인 창 안쪽 논리 좌표 x, y, 폭, 높이)
static RECT: Mutex<(f64, f64, f64, f64)> = Mutex::new((0.0, 0.0, 0.0, 0.0));
/// 아래 끝을 맞춰 띄운 팝업(확장 패널)의 아래 끝. 높이를 맞출 때 이 자리를 지킨다
static BOTTOM: Mutex<f64> = Mutex::new(0.0);

/// 지금 떠 있는 팝업의 종류 ("suggest" | "downloads" | "menu" | "bookmarks")
static KIND: Mutex<Option<String>> = Mutex::new(None);

pub fn create(app: &AppHandle, main: &Window) -> tauri::Result<()> {
    let w = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html#popup".into()))
        .data_directory(paths::webview_dir())
        .browser_extensions_enabled(true)
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
    *RECT.lock().unwrap() = (x, y, w, h);
    *BOTTOM.lock().unwrap() = y + h;
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

/// 팝업 화면이 잰 내용 높이에 창을 맞춘다. 확장 패널처럼 아래에 붙여 띄운 것은 아래 끝을 그대로 둔다.
pub fn fit(app: &AppHandle, h: f64) {
    let (Some(popup), Some(main)) = (app.get_webview_window(LABEL), app.get_window(crate::browser::MAIN_WINDOW)) else { return };
    let Some(kind) = KIND.lock().unwrap().clone() else { return };
    let (x, y, w, _) = *RECT.lock().unwrap();
    let scale = main.scale_factor().unwrap_or(1.0);
    let Ok(origin) = main.inner_position() else { return };
    let origin = origin.to_logical::<f64>(scale);
    let win_h = main.inner_size().map(|s| s.to_logical::<f64>(scale).height).unwrap_or(800.0);
    let h = h.clamp(40.0, (win_h - 16.0).max(40.0));
    let y = if kind == "extensions" { (*BOTTOM.lock().unwrap() - h).max(8.0) } else { y };
    let _ = popup.set_position(LogicalPosition::new(origin.x + x - MARGIN, origin.y + y));
    let _ = popup.set_size(LogicalSize::new(w + MARGIN * 2.0, h + MARGIN));
    *RECT.lock().unwrap() = (x, y, w, h);
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

// ───────────── 크롬 확장의 팝업 (spec 6.6) ─────────────

pub const EXT_LABEL: &str = "extpopup";
/// 처음 크기. 불러온 뒤 내용 크기에 맞춘다 (작게 시작해야 내용이 자기 폭을 드러낸다)
const EXT_START: (f64, f64) = (200.0, 120.0);
const EXT_MAX: (f64, f64) = (800.0, 600.0);

/// 확장 팝업 페이지를 띄운다. 아래 끝이 `bottom`(메인 창 안쪽 좌표)에 오게 위로 펼친다.
/// 키보드 입력을 받아야 하므로 포커스를 받는 창이고, 포커스를 잃으면 닫는다. 앱 명령 권한은 없다(capabilities에 없음).
pub fn show_extension(app: &AppHandle, url: tauri::Url, x: f64, bottom: f64) -> Result<(), String> {
    close_extension(app);
    hide(app);
    let main = app.get_window(crate::browser::MAIN_WINDOW).ok_or("브라우저 창을 찾지 못했습니다.")?;
    let scale = main.scale_factor().unwrap_or(1.0);
    let origin = main.inner_position().map_err(|e| e.to_string())?.to_logical::<f64>(scale);
    let anchor = (origin.x + x, origin.y + bottom);
    let app2 = app.clone();
    let w = WebviewWindowBuilder::new(app, EXT_LABEL, WebviewUrl::External(url))
        .data_directory(paths::webview_dir())
        .browser_extensions_enabled(true)
        .decorations(false)
        .shadow(true)
        .skip_taskbar(true)
        .resizable(false)
        .visible(false)
        .focused(!crate::dev::quiet())
        .owner_raw(main.hwnd().map_err(|e| e.to_string())?)
        .inner_size(EXT_START.0, EXT_START.1)
        .position(anchor.0, (anchor.1 - EXT_START.1).max(origin.y + 8.0))
        .on_page_load(move |_, payload| {
            if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished) {
                let a = app2.clone();
                crate::browser::later(&app2, move |_| fit_extension(&a, anchor));
            }
        })
        .build()
        .map_err(|e| format!("확장 팝업을 띄우지 못했습니다({e})."))?;
    if let Ok(h) = w.hwnd() {
        crate::dev::move_to_dev_desktop(h.0 as isize);
        unsafe {
            let _ = ShowWindow(HWND(h.0), if crate::dev::quiet() { SW_SHOWNOACTIVATE } else { windows::Win32::UI::WindowsAndMessaging::SW_SHOW });
        }
    }
    if !crate::dev::quiet() {
        let _ = w.set_focus();
    }
    // 팝업 페이지가 "지금 탭"을 물으면 답한다 (spec 6.6 호환층)
    let app4 = app.clone();
    let _ = w.with_webview(move |p| {
        if let Ok(core) = unsafe { p.controller().CoreWebView2() } {
            crate::ext_compat::listen(&app4, &core);
        }
    });
    // 한 번 포커스를 받은 뒤에 잃으면 닫는다. 만들어지는 동안의 포커스 변화로 바로 닫히지 않게 한다.
    let app3 = app.clone();
    let was_focused = std::sync::atomic::AtomicBool::new(false);
    w.on_window_event(move |e| match e {
        tauri::WindowEvent::Focused(true) => was_focused.store(true, std::sync::atomic::Ordering::Relaxed),
        tauri::WindowEvent::Focused(false) if was_focused.load(std::sync::atomic::Ordering::Relaxed) => {
            crate::browser::later(&app3, close_extension)
        }
        _ => {}
    });
    Ok(())
}

/// 팝업 내용의 크기를 재서 창을 맞춘다
fn fit_extension(app: &AppHandle, anchor: (f64, f64)) {
    let Some(w) = app.get_webview_window(EXT_LABEL) else { return };
    let app2 = app.clone();
    let _ = w.with_webview(move |p| unsafe {
        let Ok(core) = p.controller().CoreWebView2() else { return };
        let js = windows::core::w!("JSON.stringify([document.documentElement.scrollWidth, document.documentElement.scrollHeight])");
        let h = webview2_com::ExecuteScriptCompletedHandler::create(Box::new(move |err, res| {
            if err.is_ok() {
                // 결과는 JSON 글자를 다시 감싼 것이다: "\"[300,420]\""
                let inner: String = serde_json::from_str(&res).unwrap_or_default();
                if let Ok([cw, ch]) = serde_json::from_str::<[f64; 2]>(&inner) {
                    let a = app2.clone();
                    crate::browser::later(&app2, move |_| {
                        if let Some(w) = a.get_webview_window(EXT_LABEL) {
                            let (cw, ch) = (cw.clamp(EXT_START.0, EXT_MAX.0), ch.clamp(60.0, EXT_MAX.1));
                            let _ = w.set_size(LogicalSize::new(cw, ch));
                            let _ = w.set_position(LogicalPosition::new(anchor.0, (anchor.1 - ch).max(8.0)));
                        }
                    });
                }
            }
            Ok(())
        }));
        let _ = core.ExecuteScript(js, &h);
    });
}

pub fn close_extension(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(EXT_LABEL) {
        let _ = w.close();
    }
}
