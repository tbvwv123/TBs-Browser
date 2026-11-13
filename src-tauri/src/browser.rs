//! 탭과 콘텐츠 웹뷰의 수명, 배치, UI로 보내는 이벤트.
//! 웹뷰를 만들고 네이티브 객체를 다루는 일은 모두 메인 스레드에서 한다.

use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Mutex};

use tauri::{
    webview::WebviewBuilder, AppHandle, Emitter, EventTarget, LogicalPosition, LogicalSize, Manager, Url, Webview,
    WebviewUrl, Wry,
};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2Environment, COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC,
};

use crate::{
    adblock, dev, downloads, ext_compat, extensions,
    history::{now_ms, History},
    input, library, log, memory,
    native::{self, Event, MainOnly},
    paths, permissions, popup, session, shortcuts, site_scripts,
    state::{GroupId, Session, Snapshot, Tab, TabId, Tabs},
};

pub const MAIN_WINDOW: &str = "main";
pub const UI_WEBVIEW: &str = "ui";

#[derive(Default)]
pub struct Browser {
    pub tabs: Mutex<Tabs>,
    /// 페이지 카드 사각형 (논리 좌표 x, y, 폭, 높이). UI가 알려 준다.
    card: Mutex<(f64, f64, f64, f64)>,
    /// 페이지 카드 모서리 반지름 (논리 px). 0이면 각진 모서리.
    radius: Mutex<f64>,
    /// F11 전체 화면
    fullscreen: Mutex<bool>,
    /// 페이지(영상)가 켠 전체 화면. 그 탭 (spec 5.5)
    page_fullscreen: Mutex<Option<TabId>>,
    /// 사이드바를 접었는지 (spec 4.6). 세션에 남는다
    sidebar_collapsed: Mutex<bool>,
    /// 웹뷰의 빈 바탕색 (UI가 알려 준 페이지 카드 색). 없으면 흰색
    background: Mutex<Option<(u8, u8, u8)>>,
    /// 탭마다 마지막으로 남긴 방문 기록 (기록 id, 주소). 제목이 늦게 오면 이 기록을 고친다.
    visits: Mutex<HashMap<TabId, (i64, String)>>,
    /// 마지막으로 배치할 때의 활성 탭. 바뀌면 메모리 상태를 바꾼다 (spec 7장)
    last_active: Mutex<Option<TabId>>,
}

/// 마우스 옆 버튼으로 뒤로·앞으로. WebView2는 이 버튼을 처리하지 않는다.
/// 맨 위 문서는 브라우저에 알리고(홈 화면 처리 때문), 틀 안에서는 탭 기록을 바로 움직인다.
const NAV_BUTTONS_JS: &str = r#"(() => {
  if (window.__tbsNavButtons) return;
  window.__tbsNavButtons = true;
  const side = (e) => e.button === 3 || e.button === 4;
  addEventListener("mousedown", (e) => { if (side(e)) e.preventDefault(); }, true);
  addEventListener("mouseup", (e) => {
    if (!side(e)) return;
    e.preventDefault();
    e.stopPropagation();
    const dir = e.button === 3 ? "back" : "forward";
    if (window.top === window && window.chrome && chrome.webview) chrome.webview.postMessage(JSON.stringify({ __tbs_nav: dir }));
    else if (dir === "back") history.back();
    else history.forward();
  }, true);
})();"#;

/// 탭의 콘텐츠 웹뷰에 걸어 둔 것들. 웹뷰를 닫을 때 한꺼번에 뗀다 (spec 7장).
#[derive(Default)]
struct Attached {
    /// 네이티브 처리기. 걸지 못했으면 None이고, 그 탭은 웹뷰가 없는 것으로 다룬다
    hooks: Option<native::Hooks>,
    /// 웹뷰를 담은 자식 창(WRY_WEBVIEW). 둥근 모서리 영역을 여기에 건다.
    container: Option<isize>,
    /// 다운로드 처리기 토큰
    download: Option<i64>,
    /// 광고 차단 처리기 토큰 (spec 6.7)
    adblock: Option<i64>,
    /// 리소스 바꿔치기 처리기 토큰 (spec 6.2)
    overrides: Option<i64>,
}

thread_local! {
    /// 모든 웹뷰가 같이 쓰는 WebView2 환경 (UI 웹뷰의 것). 메인 스레드에만 있다.
    static ENV: RefCell<Option<ICoreWebView2Environment>> = const { RefCell::new(None) };
    /// 탭마다 콘텐츠 웹뷰에 걸어 둔 것
    static WEBVIEWS: RefCell<HashMap<TabId, Attached>> = RefCell::new(HashMap::new());
    /// "홈"으로 새 탭 화면을 덮은 탭: 그 밑의 페이지(웹뷰와 기록은 그대로 둔다). 뒤로 가면 되돌린다
    static HOME: RefCell<HashMap<TabId, Tab>> = RefCell::new(HashMap::new());
    /// 홈에서 뒤로 와서, 앞으로 가면 다시 홈이 되는 탭
    static HOME_AHEAD: RefCell<std::collections::HashSet<TabId>> = RefCell::new(std::collections::HashSet::new());
    /// 확장 페이지를 다시 연 때 (같은 탭에서 되풀이하지 않게)
    static RELOADED: RefCell<HashMap<TabId, std::time::Instant>> = RefCell::new(HashMap::new());
}

/// WebView2 이벤트 처리기가 끝난 뒤 메인 스레드에서 `f`를 돌린다.
/// 처리기 안에서 웹뷰를 만들거나 닫으면 멈출 수 있기 때문이다.
pub fn later(app: &AppHandle, f: impl FnOnce(&AppHandle) + Send + 'static) {
    let app = app.clone();
    std::thread::spawn(move || {
        let app2 = app.clone();
        let _ = app.run_on_main_thread(move || f(&app2));
    });
}

/// 모든 웹뷰가 같이 쓰는 WebView2 환경 (메인 스레드에서만)
pub fn environment() -> Option<ICoreWebView2Environment> {
    ENV.with(|e| e.borrow().clone())
}

pub fn remember_environment(env: ICoreWebView2Environment) {
    ENV.with(|e| *e.borrow_mut() = Some(env));
}

fn label(id: TabId) -> String {
    format!("tab-{id}")
}

fn webview(app: &AppHandle, id: TabId) -> Option<Webview> {
    app.get_webview(&label(id))
}

fn browser(app: &AppHandle) -> tauri::State<'_, Browser> {
    app.state::<Browser>()
}

/// 탭의 WebView2 객체 (웹뷰가 없으면 None). 메인 스레드에서만.
pub fn core_of(id: TabId) -> Option<ICoreWebView2> {
    WEBVIEWS.with(|m| m.borrow().get(&id).and_then(|a| a.hooks.as_ref()).map(|h| h.core.clone()))
}

/// 탭 웹뷰에 걸어 둔 것을 적는다
fn attached(id: TabId, f: impl FnOnce(&mut Attached)) {
    WEBVIEWS.with(|m| f(m.borrow_mut().entry(id).or_default()));
}

/// 메인 창의 핸들 (파일·폴더 고르기 창의 주인)
pub fn main_hwnd(app: &AppHandle) -> Option<isize> {
    app.get_window(MAIN_WINDOW).and_then(|w| w.hwnd().ok()).map(|h| h.0 as isize)
}

/// 탭 하나의 지금 값 (없으면 None)
fn tab_of(app: &AppHandle, id: TabId) -> Option<Tab> {
    browser(app).tabs.lock().unwrap().get(id).cloned()
}

/// 탭 하나를 고친다 (없으면 None). UI에 알리는 일은 부르는 쪽이 한다.
fn with_tab<T>(app: &AppHandle, id: TabId, f: impl FnOnce(&mut Tab) -> T) -> Option<T> {
    browser(app).tabs.lock().unwrap().get_mut(id).map(f)
}

/// 웹뷰가 있는 탭들 (버린 탭과 아직 안 불러온 탭은 빠진다)
pub fn loaded_tabs() -> Vec<TabId> {
    cores().into_iter().map(|(id, _)| id).collect()
}

/// 메모리 정책의 "버림": 웹뷰를 닫고 주소·제목·파비콘만 남긴다 (spec 7장)
pub fn discard(app: &AppHandle, id: TabId) {
    drop_webview(app, id);
    with_tab(app, id, |t| {
        t.loading = false;
        t.audible = false;
    });
    emit_tab(app, id);
}

/// "항상 유지" 표시 (탭 오른쪽 클릭 메뉴)
pub fn set_keep_alive(app: &AppHandle, id: TabId, keep: bool) {
    with_tab(app, id, |t| t.keep_alive = keep);
    emit_tab(app, id);
}

// ───────────── UI로 보내는 이벤트 ─────────────

/// UI 웹뷰에 이벤트를 보낸다
pub fn emit_ui<S: serde::Serialize + Clone>(app: &AppHandle, event: &str, payload: S) {
    let _ = app.emit_to(EventTarget::webview(UI_WEBVIEW), event, payload);
}

pub fn snapshot(app: &AppHandle) -> Snapshot {
    browser(app).tabs.lock().unwrap().snapshot()
}

fn emit_all(app: &AppHandle) {
    emit_ui(app, "tabs_changed", snapshot(app));
    session::touch();
}

fn emit_tab(app: &AppHandle, id: TabId) {
    if let Some(tab) = tab_of(app, id) {
        emit_ui(app, "tab_updated", tab);
        session::touch();
    }
}

/// 세션 파일에 남길 모양
pub fn session(app: &AppHandle) -> Session {
    let collapsed = sidebar_collapsed(app);
    browser(app).tabs.lock().unwrap().to_session(collapsed)
}

pub fn sidebar_collapsed(app: &AppHandle) -> bool {
    *browser(app).sidebar_collapsed.lock().unwrap()
}

/// 사이드바 접기·펼치기 (spec 4.6). UI가 카드 자리를 다시 알려 온다.
pub fn set_sidebar_collapsed(app: &AppHandle, collapsed: bool) {
    *browser(app).sidebar_collapsed.lock().unwrap() = collapsed;
    popup::hide(app);
    emit_ui(app, "sidebar_collapsed", collapsed);
    session::touch();
}

/// 시작할 때 저장한 탭을 되살린다. 선택된 탭만 불러오고, 나머지는 누를 때 불러온다 (spec 5.4).
pub fn restore(app: &AppHandle, s: Session) {
    *browser(app).sidebar_collapsed.lock().unwrap() = s.sidebar_collapsed;
    *browser(app).tabs.lock().unwrap() = Tabs::from_session(s);
    if browser(app).tabs.lock().unwrap().ids().is_empty() {
        new_tab(app, 0, true);
        return;
    }
    emit_all(app);
    // 환경 객체를 받은 뒤에 웹뷰를 만들도록 한 번 미룬다
    later(app, |a| {
        if let Some(id) = snapshot(a).active {
            load_if_needed(a, id);
            layout(a);
        }
    });
}

/// 주소는 있는데 아직 웹뷰가 없는 탭(복원한 탭)이면 지금 불러온다
fn load_if_needed(app: &AppHandle, id: TabId) {
    if webview(app, id).is_some() {
        return;
    }
    let url = tab_of(app, id).map(|t| t.url).unwrap_or_default();
    if url.is_empty() || input::is_internal_page(&url) {
        return;
    }
    if let Ok(u) = Url::parse(&url) {
        let _ = create_webview(app, id, Some(u), false, None);
    }
}

// ───────────── 탭 동작 ─────────────

/// 빈 탭을 `index` 자리에 만든다. 웹뷰는 처음 이동할 때 만든다 (새 탭 화면은 UI가 그린다).
pub fn new_tab(app: &AppHandle, index: usize, activate: bool) -> TabId {
    new_tab_in(app, index, None, activate)
}

fn new_tab_in(app: &AppHandle, index: usize, group: Option<GroupId>, activate: bool) -> TabId {
    let id = {
        let b = browser(app);
        let mut tabs = b.tabs.lock().unwrap();
        let id = tabs.insert_new(index, group);
        if activate {
            tabs.activate(id);
        }
        id
    };
    layout(app);
    emit_all(app);
    id
}

pub fn activate(app: &AppHandle, id: TabId) {
    if browser(app).tabs.lock().unwrap().activate(id) {
        load_if_needed(app, id);
        layout(app);
        emit_all(app);
    }
}

/// Ctrl+Tab, Ctrl+Shift+Tab
pub fn activate_neighbor(app: &AppHandle, forward: bool) {
    let next = {
        let b = browser(app);
        let tabs = b.tabs.lock().unwrap();
        tabs.active().and_then(|a| tabs.neighbor(a, forward))
    };
    if let Some(n) = next {
        activate(app, n);
    }
}

/// 닫은 탭 되살리기 (Ctrl+Shift+T)
pub fn reopen_closed(app: &AppHandle) {
    let reopened = browser(app).tabs.lock().unwrap().reopen_closed();
    if let Some((id, _)) = reopened {
        load_if_needed(app, id);
        layout(app);
        emit_all(app);
    }
}

/// 내부 페이지(방문 기록, 다운로드, 설정)를 연다. 이미 열린 탭이 있으면 그 탭으로 간다.
pub fn open_internal(app: &AppHandle, page: &str) {
    let existing = snapshot(app).tabs.into_iter().find(|t| t.url == page).map(|t| t.id);
    match existing {
        Some(id) => activate(app, id),
        None => {
            let id = new_tab(app, 0, true);
            if let Ok(u) = Url::parse(page) {
                let _ = navigate(app, id, u);
            }
        }
    }
}

/// Ctrl+D: 현재 페이지를 북마크에 넣는다 (이미 있으면 그대로 둔다)
pub fn bookmark_active(app: &AppHandle) {
    let snap = snapshot(app);
    let Some(t) = snap.active.and_then(|a| snap.tabs.into_iter().find(|t| t.id == a)) else { return };
    if t.url.is_empty() || input::is_internal_page(&t.url) {
        return;
    }
    let _ = library::edit(app, |b| {
        if b.find_url(&t.url).is_none() {
            b.add_link(None, None, t.title.clone(), t.url.clone(), t.favicon.clone())?;
        }
        Ok(())
    });
}

/// F11: 사이드바와 툴바를 숨기고 페이지를 창 전체로 키운다 (UI가 카드 자리를 다시 알린다).
/// 영상이 켠 전체 화면 중이면 그것을 끈다.
pub fn toggle_fullscreen(app: &AppHandle) {
    let page = *browser(app).page_fullscreen.lock().unwrap();
    if let Some(t) = page {
        if let Some(c) = core_of(t) {
            native::exit_fullscreen(&c);
        }
        set_page_fullscreen(app, t, false);
        return;
    }
    {
        let b = browser(app);
        let mut f = b.fullscreen.lock().unwrap();
        *f = !*f;
    }
    apply_fullscreen(app);
}

/// 페이지가 전체 화면을 켜거나 껐다 (spec 5.5). 보고 있는 탭의 요청만 따른다.
pub(crate) fn set_page_fullscreen(app: &AppHandle, id: TabId, on: bool) {
    {
        let b = browser(app);
        let mut p = b.page_fullscreen.lock().unwrap();
        if on {
            if b.tabs.lock().unwrap().active() != Some(id) {
                return;
            }
            *p = Some(id);
        } else if *p == Some(id) {
            *p = None;
        } else {
            return;
        }
    }
    apply_fullscreen(app);
}

/// F11이나 페이지 전체 화면 중 하나라도 켜져 있으면 창을 전체 화면으로
fn apply_fullscreen(app: &AppHandle) {
    let on = is_fullscreen(app);
    if let Some(w) = app.get_window(MAIN_WINDOW) {
        if w.is_fullscreen().unwrap_or(false) != on {
            let _ = w.set_fullscreen(on);
            // 창을 tao를 거치지 않고 보였으면(개발용 데스크톱) tao가 창 모양을 다시 쓰면서 보임 표시를 뺀다. 활성화 없이 다시 보인다
            if let Ok(h) = w.hwnd() {
                use windows::Win32::UI::WindowsAndMessaging::{IsWindowVisible, ShowWindow, SW_SHOWNA};
                unsafe {
                    if !IsWindowVisible(windows::Win32::Foundation::HWND(h.0)).as_bool() {
                        let _ = ShowWindow(windows::Win32::Foundation::HWND(h.0), SW_SHOWNA);
                    }
                }
            }
        }
    }
    popup::hide(app);
    emit_ui(app, "fullscreen", on);
}

pub fn is_fullscreen(app: &AppHandle) -> bool {
    let b = browser(app);
    let f11 = *b.fullscreen.lock().unwrap();
    f11 || b.page_fullscreen.lock().unwrap().is_some()
}

/// UI가 그리는 테마 (spec 4.3). 웹 페이지의 prefers-color-scheme과 새 웹뷰의 빈 바탕을 맞춘다.
/// `system`이면 페이지도 Windows 설정을 따른다.
pub fn apply_theme(app: &AppHandle, dark: bool, system: bool, background: (u8, u8, u8)) {
    *browser(app).background.lock().unwrap() = Some(background);
    let scheme = if system { "auto" } else if dark { "dark" } else { "light" };
    // 프로필 하나를 모든 웹뷰가 같이 쓴다. 아무 웹뷰에서나 한 번 걸면 된다.
    if let Some(ui) = app.get_webview(UI_WEBVIEW) {
        let _ = ui.with_webview(move |p| {
            if let Ok(c) = unsafe { p.controller().CoreWebView2() } {
                native::set_color_scheme(&c, scheme);
            }
        });
    }
    WEBVIEWS.with(|m| {
        for hk in m.borrow().values().filter_map(|a| a.hooks.as_ref()) {
            native::set_background(&hk.controller, background);
        }
    });
}

/// UI에 포커스가 있을 때 Ctrl+F: 페이지로 포커스를 옮긴다 (페이지 안 찾기는 WebView2 기본 기능)
pub fn focus_page(app: &AppHandle) {
    if let Some(wv) = snapshot(app).active.and_then(|a| webview(app, a)) {
        let _ = wv.set_focus();
    }
}

/// UI에 포커스가 있을 때 F12: 지금 보는 페이지의 개발자 도구
pub fn open_devtools(app: &AppHandle) {
    if let Some(c) = snapshot(app).active.and_then(core_of) {
        let _ = unsafe { c.OpenDevToolsWindow() };
    }
}

pub fn close(app: &AppHandle, id: TabId) {
    let empty = {
        let b = browser(app);
        let mut tabs = b.tabs.lock().unwrap();
        tabs.close(id);
        tabs.ids().is_empty()
    };
    drop_webview(app, id);
    browser(app).visits.lock().unwrap().remove(&id);
    // 웹뷰를 닫아도 남겨 두는 것(홈 밑의 페이지 등)은 탭을 닫을 때 지운다
    HOME.with(|h| h.borrow_mut().remove(&id));
    HOME_AHEAD.with(|h| h.borrow_mut().remove(&id));
    RELOADED.with(|r| r.borrow_mut().remove(&id));
    if empty {
        // 마지막 탭을 닫으면 새 탭 하나를 연다 (spec 5.1)
        new_tab(app, 0, true);
    } else {
        layout(app);
        emit_all(app);
    }
}

/// 목록 맨 위에 새 탭을 열고 그 주소로 간다 (북마크를 Ctrl+클릭, 가운데 클릭)
pub fn open_in_new_tab(app: &AppHandle, url: Url) -> Result<(), String> {
    let id = new_tab(app, 0, true);
    navigate(app, id, url)
}

/// 탭 목록이나 그룹만 바뀌는 일. 바꾼 뒤 배치와 UI를 맞춘다.
pub fn edit_tabs(app: &AppHandle, f: impl FnOnce(&mut Tabs)) {
    {
        let b = browser(app);
        let mut tabs = b.tabs.lock().unwrap();
        f(&mut tabs);
    }
    layout(app);
    emit_all(app);
}

pub fn navigate(app: &AppHandle, id: TabId, url: Url) -> Result<(), String> {
    let internal = input::is_internal_page(url.as_str());
    HOME.with(|h| h.borrow_mut().remove(&id));
    HOME_AHEAD.with(|h| h.borrow_mut().remove(&id));
    with_tab(app, id, |tab| {
        if internal {
            tab.clear_page();
        }
        tab.url = url.to_string();
        tab.loading = !internal;
    })
    .ok_or("탭을 찾지 못했습니다. 탭 목록에서 다시 골라 주세요.")?;
    emit_tab(app, id);
    if internal {
        // 내부 페이지는 UI가 카드 자리에 그린다. 콘텐츠 웹뷰는 쓰지 않는다 (spec 3장)
        drop_webview(app, id);
        layout(app);
        return Ok(());
    }
    match webview(app, id) {
        Some(wv) => {
            wv.navigate(url).map_err(|e| format!("페이지로 이동하지 못했습니다({e}). 주소를 확인하고 다시 시도해 주세요."))?;
            if !dev::quiet() {
                let _ = wv.set_focus();
            }
        }
        None => create_webview(app, id, Some(url), !dev::quiet(), None)?,
    }
    Ok(())
}

/// 탭의 콘텐츠 웹뷰만 닫는다 (탭은 남는다). 걸어 둔 처리기를 먼저 뗀다 (spec 7장).
fn drop_webview(app: &AppHandle, id: TabId) {
    permissions::drop_tab(app, id);
    let was_full = {
        let b = browser(app);
        let mut p = b.page_fullscreen.lock().unwrap();
        let hit = *p == Some(id);
        if hit {
            *p = None;
        }
        hit
    };
    if was_full {
        later(app, apply_fullscreen);
    }
    let attached = WEBVIEWS.with(|m| m.borrow_mut().remove(&id)).unwrap_or_default();
    if let Some(h) = attached.hooks {
        if let Some(t) = attached.download {
            downloads::detach(&h.core, t);
        }
        if let Some(t) = attached.adblock {
            adblock::detach(&h.core, id, t);
        }
        if let Some(t) = attached.overrides {
            site_scripts::detach_overrides(&h.core, t);
        }
        native::unhook(&h);
    }
    site_scripts::forget(id);
    memory::forget(id);
    if let Some(wv) = webview(app, id) {
        let _ = wv.close();
    }
}

/// 탭마다 남겨 둔 것의 (이름, 탭 id). 개발용 `orphans` 확인에 쓴다 (메인 스레드에서만)
#[cfg(debug_assertions)]
pub fn tab_entries(app: &AppHandle) -> Vec<(&'static str, TabId)> {
    let mut v: Vec<(&'static str, TabId)> = Vec::new();
    WEBVIEWS.with(|m| v.extend(m.borrow().keys().map(|id| ("browser.WEBVIEWS", *id))));
    HOME.with(|m| v.extend(m.borrow().keys().map(|id| ("browser.HOME", *id))));
    HOME_AHEAD.with(|m| v.extend(m.borrow().iter().map(|id| ("browser.HOME_AHEAD", *id))));
    RELOADED.with(|m| v.extend(m.borrow().keys().map(|id| ("browser.RELOADED", *id))));
    let b = browser(app);
    v.extend(b.visits.lock().unwrap().keys().map(|id| ("browser.visits", *id)));
    v.extend(b.page_fullscreen.lock().unwrap().map(|id| ("browser.page_fullscreen", id)));
    v.extend(b.last_active.lock().unwrap().map(|id| ("browser.last_active", id)));
    v
}

/// 웹뷰가 있는 탭들 (메인 스레드에서만)
pub fn cores() -> Vec<(TabId, ICoreWebView2)> {
    WEBVIEWS.with(|m| m.borrow().iter().filter_map(|(id, a)| Some((*id, a.hooks.as_ref()?.core.clone()))).collect())
}

/// 사이트 스크립트를 켜고 끄거나 다시 불러온 뒤, 열린 탭마다 등록을 새로 한다.
/// 열린 탭은 새로고침 뒤에 반영된다 (spec 6.2). 바꿔치기가 있으면 디스크 캐시를 비운다.
pub fn reapply_scripts(app: &AppHandle, clear_cache: bool) {
    let cores = cores();
    for (id, c) in &cores {
        site_scripts::apply(app, *id, c, Box::new(|| {}));
    }
    if clear_cache {
        if let Some((_, c)) = cores.first() {
            site_scripts::clear_disk_cache(c);
        }
    }
}

pub fn go_back(app: &AppHandle, id: TabId) {
    if let Some(under) = HOME.with(|h| h.borrow_mut().remove(&id)) {
        // 홈에서 뒤로: 밑의 페이지를 되돌린다
        HOME_AHEAD.with(|h| h.borrow_mut().insert(id));
        let url = under.url.clone();
        with_tab(app, id, |t| {
            t.url = under.url;
            t.title = under.title;
            t.favicon = under.favicon;
            t.can_go_back = under.can_go_back;
            t.can_go_forward = true;
        });
        // 메모리 관리로 웹뷰를 버렸으면 다시 연다
        if webview(app, id).is_none() {
            if let Ok(u) = Url::parse(&url) {
                let _ = navigate(app, id, u);
            }
        }
        layout(app);
        emit_tab(app, id);
        return;
    }
    if let Some(c) = core_of(id) {
        let _ = unsafe { c.GoBack() };
    }
}

pub fn go_forward(app: &AppHandle, id: TabId) {
    let can = tab_of(app, id).is_some_and(|t| t.can_go_forward);
    if HOME_AHEAD.with(|h| h.borrow().contains(&id)) && !core_of(id).is_some_and(|c| native::history(&c).1) && can {
        return go_home(app, id);
    }
    if let Some(c) = core_of(id) {
        let _ = unsafe { c.GoForward() };
    }
}

/// "홈": 이 탭에 새 탭 화면을 띄운다. 웹 페이지의 웹뷰와 기록은 남겨 두어 뒤로 가면 돌아온다
pub fn go_home(app: &AppHandle, id: TabId) {
    let Some(tab) = tab_of(app, id) else { return };
    if tab.url.is_empty() || at_home(id) {
        return;
    }
    if webview(app, id).is_none() || input::is_internal_page(&tab.url) {
        return to_blank(app, id);
    }
    HOME_AHEAD.with(|h| h.borrow_mut().remove(&id));
    HOME.with(|h| h.borrow_mut().insert(id, tab));
    with_tab(app, id, |t| {
        t.clear_page();
        t.can_go_back = true;
    });
    layout(app);
    emit_tab(app, id);
}

fn at_home(id: TabId) -> bool {
    HOME.with(|h| h.borrow().contains_key(&id))
}

pub fn stop(id: TabId) {
    if let Some(c) = core_of(id) {
        let _ = unsafe { c.Stop() };
    }
}

pub fn reload(app: &AppHandle, id: TabId) {
    if let Some(wv) = webview(app, id) {
        let _ = wv.reload();
    }
}

pub fn set_card(app: &AppHandle, x: f64, y: f64, w: f64, h: f64, radius: f64) {
    *browser(app).card.lock().unwrap() = (x, y, w.max(1.0), h.max(1.0));
    *browser(app).radius.lock().unwrap() = radius.max(0.0);
    layout(app);
}

/// 웹뷰 자식 창을 둥근 사각형으로 자른다 (spec 3장 배치, 0단계 4번). 가장자리는 매끄럽지 않다.
/// `with_webview` 콜백 안에서도 부르므로 Tauri API는 쓰지 않는다 (콜백 안에서 부르면 멈춘다).
fn clip_corners(app: &AppHandle, id: TabId) {
    use windows::Win32::{
        Foundation::HWND,
        Graphics::Gdi::{CreateRoundRectRgn, SetWindowRgn},
        UI::HiDpi::GetDpiForWindow,
    };
    let Some(hwnd) = WEBVIEWS.with(|m| m.borrow().get(&id).and_then(|a| a.container)) else { return };
    let (_, _, w, h) = *browser(app).card.lock().unwrap();
    let r = *browser(app).radius.lock().unwrap();
    let dpi = unsafe { GetDpiForWindow(HWND(hwnd as _)) };
    let s = if dpi == 0 { 1.0 } else { f64::from(dpi) / 96.0 };
    let (pw, ph, pr) = ((w * s).round() as i32, (h * s).round() as i32, (r * s).round() as i32);
    unsafe {
        if pr <= 0 {
            SetWindowRgn(HWND(hwnd as _), None, true);
        } else {
            // 영역의 주인은 창이 된다. 따로 지우지 않는다.
            let rgn = CreateRoundRectRgn(0, 0, pw + 1, ph + 1, pr * 2, pr * 2);
            SetWindowRgn(HWND(hwnd as _), Some(rgn), true);
        }
    }
}

/// 웹뷰를 카드 자리에 놓고, 활성 탭만 보인다.
/// 활성 탭이 바뀌었으면 전 탭은 백그라운드로, 새 탭은 깨우거나 (버렸으면) 다시 불러온다.
fn layout(app: &AppHandle) {
    let (x, y, w, h) = *browser(app).card.lock().unwrap();
    let (ids, active) = {
        let b = browser(app);
        let tabs = b.tabs.lock().unwrap();
        (tabs.ids(), tabs.active())
    };
    let prev = std::mem::replace(&mut *browser(app).last_active.lock().unwrap(), active);
    if prev != active {
        // 확장 팝업은 연 탭 기준으로 그려지므로 탭을 바꾸면 닫는다
        later(app, popup::close_extension);
        // 전체 화면이던 영상 탭에서 떠나면 그 전체 화면을 끈다
        let page = *browser(app).page_fullscreen.lock().unwrap();
        if let Some(t) = page.filter(|t| Some(*t) != active) {
            if let Some(c) = core_of(t) {
                native::exit_fullscreen(&c);
            }
            *browser(app).page_fullscreen.lock().unwrap() = None;
            later(app, apply_fullscreen);
        }
        memory::on_switch(prev, active);
        if let Some(a) = active {
            load_if_needed(app, a);
        }
    }
    for id in ids {
        if let Some(wv) = webview(app, id) {
            if Some(id) == active && !at_home(id) {
                let _ = wv.set_position(LogicalPosition::new(x, y));
                let _ = wv.set_size(LogicalSize::new(w, h));
                clip_corners(app, id);
                let _ = wv.show();
            } else {
                let _ = wv.hide();
            }
        }
    }
}

// ───────────── 콘텐츠 웹뷰 ─────────────

type AfterCreate = MainOnly<Box<dyn FnOnce(&ICoreWebView2)>>;

fn content_builder(label: &str) -> WebviewBuilder<Wry> {
    // 빈 페이지로 만들고, 사이트 스크립트를 등록한 뒤에 첫 이동을 한다 (spec 6.2)
    let mut b = WebviewBuilder::new(label, WebviewUrl::External(Url::parse("about:blank").unwrap()))
        .data_directory(paths::webview_dir())
        // 콘텐츠 웹뷰는 앱 내부 주소로 가지 못한다 (spec 3장 보안 경계)
        .on_navigation(|u| !input::is_app_internal(u))
        // 크롬 확장 허용 (spec 6.6). 모든 웹뷰가 같은 환경 옵션이어야 한다
        .browser_extensions_enabled(true)
        .focused(false);
    if let Some(env) = ENV.with(|e| e.borrow().clone()) {
        b = b.with_environment(env);
    }
    b
}

/// 탭의 콘텐츠 웹뷰를 만든다. `url`이 있으면 사이트 스크립트를 등록한 뒤 그리로 간다.
/// `after`는 새 창 요청(팝업)을 넘겨받을 때 쓴다.
fn create_webview(app: &AppHandle, id: TabId, url: Option<Url>, focus: bool, after: Option<AfterCreate>) -> Result<(), String> {
    let win = app
        .get_window(MAIN_WINDOW)
        .ok_or("브라우저 창을 찾지 못했습니다. 프로그램을 다시 시작해 주세요.")?;
    let (x, y, w, h) = *browser(app).card.lock().unwrap();
    let wv = win
        .add_child(content_builder(&label(id)), LogicalPosition::new(x, y), LogicalSize::new(w, h))
        .map_err(|e| format!("탭을 만들지 못했습니다({e}). 탭을 닫고 다시 열어 주세요."))?;
    layout(app);
    let app2 = app.clone();
    let _ = wv.with_webview(move |p| {
        let Ok(core) = (unsafe { p.controller().CoreWebView2() }) else { return };
        native::enable_autofill(&core);
        native::set_background(&p.controller(), browser(&app2).background.lock().unwrap().unwrap_or((255, 255, 255)));
        let mut container = windows::Win32::Foundation::HWND::default();
        if unsafe { p.controller().ParentWindow(&mut container) }.is_ok() {
            attached(id, |a| a.container = Some(container.0 as isize));
            // 여기서 layout()처럼 Tauri의 웹뷰 API를 부르면 멈춘다. Win32로만 자른다.
            clip_corners(&app2, id);
        }
        let app3 = app2.clone();
        let sink: Rc<dyn Fn(Event)> = Rc::new(move |ev| on_event(&app3, id, ev));
        match native::hook(&p.controller(), &core, sink) {
            Ok(h) => attached(id, |a| a.hooks = Some(h)),
            Err(e) => log::error(format!("탭 {id}의 처리기를 걸지 못함: {e:?}")),
        }
        let token = downloads::attach(&app2, &core, id);
        attached(id, |a| a.download = token);
        // 마우스 옆 버튼(뒤로·앞으로)
        unsafe {
            let _ = core.AddScriptToExecuteOnDocumentCreated(&windows::core::HSTRING::from(NAV_BUTTONS_JS), None);
        }
        // 광고 차단 (spec 6.7): 요청 처리기와 화면 숨기기 스크립트
        let token = adblock::attach(&app2, &core, p.environment(), id);
        attached(id, |a| a.adblock = token);
        let token = site_scripts::attach_overrides(&app2, &core, p.environment());
        attached(id, |a| a.overrides = token);
        if focus {
            let _ = unsafe { p.controller().MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC) };
        }
        let core2 = core.clone();
        let app4 = app2.clone();
        site_scripts::apply(
            &app2,
            id,
            &core,
            Box::new(move || {
                if let Some(u) = url {
                    let c = core2.clone();
                    adblock::when_ready(
                        &app4,
                        Box::new(move || {
                            native::navigate(&c, u.as_str());
                        }),
                    );
                }
                if let Some(after) = after {
                    (after.0)(&core2);
                }
            }),
        );
    });
    Ok(())
}

fn on_event(app: &AppHandle, id: TabId, ev: Event) {
    match ev {
        Event::NewWindow { uri, args, deferral } => return open_new_window(app, id, uri, MainOnly((args, deferral))),
        Event::CloseRequested => return close_later(app, id),
        Event::Shortcut(name) => return later(app, move |a| shortcuts::run(a, name)),
        Event::ScriptShortcut(script, sc) => {
            return later(app, move |_| {
                if let Some(c) = core_of(id) {
                    site_scripts::dispatch(&c, &script, "shortcut", &sc);
                }
            })
        }
        Event::Message(_, text) if text.starts_with("{\"__tbs_nav\":") => {
            let back = text.contains("\"back\"");
            return later(app, move |a| if back { go_back(a, id) } else { go_forward(a, id) });
        }
        Event::Message(source, text) => {
            if let Some(c) = core_of(id) {
                // 탭에 연 확장 페이지(설정 등)의 호환 메시지 (spec 6.6)
                if !adblock::handle_message(&c, &source, &text) && !ext_compat::handle_message(app, &c, &source, &text) {
                    site_scripts::handle_message(app, id, &c, &source, &text);
                }
            }
            return;
        }
        Event::Navigating(uri) => {
            if let Some(c) = core_of(id) {
                adblock::navigating(app, id, &c, &uri);
            }
            return;
        }
        // 확장이 만든 오른쪽 클릭 메뉴 항목을 넣는다 (spec 6.6)
        Event::ContextMenu(args) => return ext_compat::extend_menu(app, id, &args),
        Event::Completed(ok) => {
            if ok {
                record_visit(app, id);
            }
            return;
        }
        // 확장을 끄거나 바꾸면 WebView2가 그 확장의 페이지를 자기 빈 페이지로 보낸다. 그 페이지를 다시 연다.
        Event::Url(u, _) if u.starts_with("chrome-search://") => return later(app, move |a| extension_page_lost(a, id)),
        Event::FullScreen(on) => return later(app, move |a| set_page_fullscreen(a, id, on)),
        Event::Permission { uri, kind, args } => return permissions::request(app, id, &uri, kind, args),
        Event::GotFocus => {
            // 페이지를 누르면 떠 있는 팝업(메뉴, 북마크 목록 등)을 닫는다
            // 설치·권한 확인은 바깥을 눌러도 닫지 않는다 (시안 M)
            if popup::is_open() && !popup::is_showing("install") {
                later(app, popup::hide);
            }
            return;
        }
        _ => {}
    }
    if at_home(id) && matches!(ev, Event::Url(..) | Event::History { .. } | Event::Loading(_) | Event::Title(_) | Event::Favicon(_)) {
        return;
    }
    if let Event::Url(_, false) = ev {
        HOME_AHEAD.with(|h| h.borrow_mut().remove(&id));
    }
    let mut same_document = false;
    let mut new_document = false;
    let mut favicon = None;
    let mut new_title = None;
    {
        let b = browser(app);
        let mut tabs = b.tabs.lock().unwrap();
        let Some(tab) = tabs.get_mut(id) else { return };
        match ev {
            Event::Url(u, same_doc) => {
                tab.url = u;
                same_document = same_doc;
                new_document = !same_doc;
            }
            Event::History { back, forward } => {
                tab.can_go_back = back;
                tab.can_go_forward = forward;
            }
            Event::Loading(l) => {
                if l {
                    // 새 페이지: 막은 개수를 0부터
                    adblock::page_started(id);
                }
                tab.loading = l
            }
            Event::Title(t) => {
                new_title = Some((tab.url.clone(), t.clone()));
                tab.title = t;
            }
            Event::Favicon(f) => {
                if let Some(data) = &f {
                    favicon = Some((tab.url.clone(), data.clone()));
                }
                tab.favicon = f;
            }
            Event::Audio(a) => tab.audible = a,
            _ => unreachable!(),
        }
    }
    emit_tab(app, id);
    // 다른 문서로 가면 앞 문서가 하던 권한 요청은 거절로 끝낸다
    if new_document {
        permissions::drop_tab(app, id);
    }
    if let Some((url, data)) = favicon {
        let _ = app.state::<History>().set_favicon(&url, &data);
    }
    // 문서를 새로 불러오지 않은 이동(pushState)도 기록한다
    if same_document {
        record_visit(app, id);
    }
    if let Some((url, title)) = new_title {
        let last = browser(app).visits.lock().unwrap().get(&id).cloned();
        if let Some((vid, vurl)) = last {
            if vurl == url {
                let _ = app.state::<History>().set_title(vid, &title);
            }
        }
    }
}

/// 확장 페이지를 연 탭이 WebView2의 빈 페이지로 넘어갔다 (확장을 껐거나, 업데이트하며 껐다 켰다).
/// 확장이 켜져 있으면 그 페이지를 다시 연다. 꺼져 있으면 주소를 남긴 채 웹뷰만 닫고, 다시 켤 때 연다.
fn extension_page_lost(app: &AppHandle, id: TabId) {
    let url = tab_of(app, id).map(|t| t.url).unwrap_or_default();
    let Some(ext) = url.strip_prefix("chrome-extension://").and_then(|r| r.split('/').next()).map(str::to_string) else {
        return to_blank(app, id);
    };
    let recent = RELOADED.with(|r| r.borrow().get(&id).is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(5)));
    if extensions::is_enabled(app, &ext) && !recent {
        RELOADED.with(|r| r.borrow_mut().insert(id, std::time::Instant::now()));
        if let (Some(c), Ok(_)) = (core_of(id), Url::parse(&url)) {
            native::navigate(&c, &url);
        }
        return;
    }
    drop_webview(app, id);
    with_tab(app, id, |t| t.loading = false);
    layout(app);
    emit_tab(app, id);
}

/// 확장을 다시 켜거나 새로 설치했다. 그 확장의 페이지를 연 탭을 다시 연다 (지금 보는 탭만 바로, 나머지는 누를 때)
pub fn reload_extension_tabs(app: &AppHandle, ext: &str) {
    let prefix = format!("chrome-extension://{ext}/");
    let snap = snapshot(app);
    for t in snap.tabs.iter().filter(|t| t.url.starts_with(&prefix)) {
        match core_of(t.id) {
            Some(c) => native::navigate(&c, &t.url),
            None if Some(t.id) == snap.active => load_if_needed(app, t.id),
            None => {}
        }
    }
}

/// 탭을 빈 탭(새 탭 화면)으로 되돌린다. 웹뷰는 닫는다.
pub fn to_blank(app: &AppHandle, id: TabId) {
    if with_tab(app, id, Tab::clear_page).is_none() {
        return;
    }
    drop_webview(app, id);
    layout(app);
    emit_tab(app, id);
}

/// 이동이 끝난 페이지의 주소, 제목, 시각을 남긴다 (spec 5.4). 같은 탭에서 같은 주소가 이어지면 한 번만.
fn record_visit(app: &AppHandle, id: TabId) {
    let Some(tab) = tab_of(app, id) else { return };
    if tab.url.is_empty() || tab.url == "about:blank" || tab.url.starts_with("chrome-extension:") || input::is_internal_page(&tab.url) {
        return;
    }
    let last = browser(app).visits.lock().unwrap().get(&id).map(|(_, u)| u.clone());
    if last.as_deref() == Some(tab.url.as_str()) {
        return;
    }
    if let Ok(vid) = app.state::<History>().add_visit(&tab.url, &tab.title, now_ms()) {
        browser(app).visits.lock().unwrap().insert(id, (vid, tab.url));
    }
}

/// 탭을 닫되, WebView2 이벤트 처리기가 끝난 뒤에 닫는다 (자기 처리기 안에서 웹뷰를 닫지 않는다).
/// 팝업으로 연 로그인 창 등이 `window.close()`를 부르면 그 탭을 닫는다. 닫은 뒤에는 바로 아래 탭,
/// 곧 팝업을 연 탭이 선택된다 (팝업은 연 탭 바로 위에 생기므로).
fn close_later(app: &AppHandle, id: TabId) {
    later(app, move |a| close(a, id));
}

/// 페이지가 새 창을 요청하거나 링크를 가운데 클릭하면, 현재 탭 바로 위에 새 탭으로 연다 (spec 5.1).
/// 새 탭을 요청한 페이지의 새 창으로 넘겨서 `window.opener`가 살아 있게 한다 (팝업 로그인).
fn open_new_window(
    app: &AppHandle,
    opener: TabId,
    uri: String,
    req: MainOnly<(
        webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2NewWindowRequestedEventArgs,
        webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Deferral,
    )>,
) {
    // WebView2 이벤트 처리기 안에서 웹뷰를 만들면 멈출 수 있어, 처리기가 끝난 뒤 메인 스레드에서 한다.
    later(app, move |app| {
        let req = req;
        let (index, group) = {
            let b = browser(app);
            let tabs = b.tabs.lock().unwrap();
            (tabs.index_of(opener).unwrap_or(0), tabs.get(opener).and_then(|t| t.group))
        };
        // 그룹 안의 탭에서 연 새 탭은 같은 그룹에 들어간다
        let id = new_tab_in(app, index, group, true);
        let fallback_url = Url::parse(&uri).ok();
        let app2 = app.clone();
        let after: AfterCreate = MainOnly(Box::new(move |core: &ICoreWebView2| {
            let (args, deferral) = req.0;
            let ok = unsafe { args.SetNewWindow(core).is_ok() && args.SetHandled(true).is_ok() };
            let _ = unsafe { deferral.Complete() };
            if !ok {
                log::error(format!("새 창 넘기기 실패, 주소로 다시 연다: {uri}"));
                if let Some(u) = Url::parse(&uri).ok().filter(|u| !input::is_app_internal(u)) {
                    // 콜백 밖에서 이동한다
                    later(&app2, move |a| {
                        let _ = navigate(a, id, u);
                    });
                }
            }
        }));
        if let Err(e) = create_webview(app, id, None, false, Some(after)) {
            log::error(format!("{e}"));
            if let Some(u) = fallback_url {
                let _ = navigate(app, id, u);
            }
        }
    });
}
