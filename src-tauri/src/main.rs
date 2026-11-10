// 릴리스 빌드에서는 콘솔 창을 띄우지 않는다
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bookmarks;
mod browser;
mod dev;
mod downloads;
mod examples;
mod extensions;
mod frame;
mod history;
mod input;
mod library;
mod log;
mod memory;
mod native;
mod paths;
mod permissions;
mod popup;
mod scripts;
mod session;
mod settings;
mod shortcuts;
mod site_scripts;
mod state;
mod storage;
mod updater;

use tauri::{
    webview::WebviewBuilder, window::WindowBuilder, AppHandle, Emitter, EventTarget, LogicalPosition, LogicalSize, Manager, WebviewUrl,
};

use browser::{Browser, MAIN_WINDOW, UI_WEBVIEW};
use bookmarks::Bookmarks;
use history::{Download, History, Suggestion, TopSite, Visit};
use settings::{Settings, SettingsStore};
use state::{GroupId, Snapshot, TabId};

/// 화면에 보이는 이름. 이름을 바꿀 때는 이 상수와 APP_DIR_NAME만 고친다.
pub const APP_NAME: &str = "TB's Browser";
/// 파일·폴더 이름처럼 아포스트로피를 쓸 수 없는 곳의 이름.
pub const APP_DIR_NAME: &str = "TBs Browser";

// ───────────── UI → 호스트 명령 (UI 웹뷰에만 허용, capabilities/ui.json) ─────────────
//
// 명령은 UI 웹뷰의 WebView2 이벤트 처리기 안에서 불린다. 그 안에서 웹뷰를 만들면 메인 스레드가 멈추므로,
// 웹뷰를 건드리는 명령은 async로 두고 실제 일은 `on_main`으로 메인 스레드 이벤트 루프에 넘긴다.

/// 메인 스레드 이벤트 루프에서 `f`를 돌리고 결과를 기다린다. async 명령에서만 부른다.
fn on_main<T: Send + 'static>(app: &AppHandle, f: impl FnOnce(&AppHandle) -> T + Send + 'static) -> Result<T, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let a = app.clone();
    app.run_on_main_thread(move || {
        let _ = tx.send(f(&a));
    })
    .map_err(|e| format!("브라우저 내부 작업을 시작하지 못했습니다({e}). 프로그램을 다시 시작해 주세요."))?;
    rx.recv().map_err(|_| "브라우저 내부 작업이 끝나지 않았습니다. 프로그램을 다시 시작해 주세요.".to_string())
}

#[tauri::command]
fn get_state(app: AppHandle) -> Snapshot {
    browser::snapshot(&app)
}

#[tauri::command]
async fn tab_create(app: AppHandle) -> Result<TabId, String> {
    // 새 탭은 목록 맨 위에 쌓인다 (spec 5.1)
    on_main(&app, |a| browser::new_tab(a, 0, true))
}

#[tauri::command]
async fn tab_close(app: AppHandle, id: TabId) -> Result<(), String> {
    on_main(&app, move |a| browser::close(a, id))
}

#[tauri::command]
async fn tab_activate(app: AppHandle, id: TabId) -> Result<(), String> {
    on_main(&app, move |a| browser::activate(a, id))
}

#[tauri::command]
async fn navigate(app: AppHandle, id: TabId, input: String) -> Result<(), String> {
    let url = input::resolve(&input, settings::get(&app).search_engine).ok_or("주소나 검색어를 입력해 주세요.")?;
    on_main(&app, move |a| browser::navigate(a, id, url))?
}

#[tauri::command]
async fn go_back(app: AppHandle, id: TabId) -> Result<(), String> {
    on_main(&app, move |_| browser::go_back(id))
}

#[tauri::command]
async fn go_forward(app: AppHandle, id: TabId) -> Result<(), String> {
    on_main(&app, move |_| browser::go_forward(id))
}

#[tauri::command]
async fn reload(app: AppHandle, id: TabId) -> Result<(), String> {
    on_main(&app, move |a| browser::reload(a, id))
}

#[tauri::command]
async fn stop(app: AppHandle, id: TabId) -> Result<(), String> {
    on_main(&app, move |_| browser::stop(id))
}

/// 페이지 카드 사각형과 모서리 반지름(tokens.css의 --card-radius)을 UI가 알린다.
#[tauri::command]
async fn set_content_bounds(app: AppHandle, x: f64, y: f64, w: f64, h: f64, radius: f64) -> Result<(), String> {
    on_main(&app, move |a| browser::set_card(a, x, y, w, h, radius))
}

/// 최대화 버튼 자리를 UI가 알린다. 그 자리에 스냅 레이아웃용 덮개 창을 놓는다.
#[tauri::command]
async fn set_max_button_bounds(app: AppHandle, x: f64, y: f64, w: f64, h: f64) -> Result<(), String> {
    on_main(&app, move |a| {
        if let Some(win) = a.get_window(MAIN_WINDOW) {
            frame::place_max_button(&win, x, y, w, h);
        }
    })
}

// ───────────── 탭 그룹, 탭 옮기기 (spec 5.1) ─────────────

/// 주소나 검색어를 새 탭(목록 맨 위)에서 연다. 북마크를 Ctrl+클릭하거나 가운데 클릭할 때 쓴다.
#[tauri::command]
async fn tab_open_url(app: AppHandle, input: String) -> Result<(), String> {
    let url = input::resolve(&input, settings::get(&app).search_engine).ok_or("열 주소가 비어 있습니다.")?;
    on_main(&app, move |a| browser::open_in_new_tab(a, url))?
}

/// 탭을 `before` 탭 앞(없으면 맨 끝)으로 옮기고 `group`에 넣는다.
#[tauri::command]
async fn tab_move(app: AppHandle, id: TabId, before: Option<TabId>, group: Option<GroupId>) -> Result<(), String> {
    on_main(&app, move |a| browser::edit_tabs(a, |t| {
        t.move_tab(id, before, group);
    }))
}

#[tauri::command]
async fn group_create(app: AppHandle, tab: TabId, title: String) -> Result<Option<GroupId>, String> {
    on_main(&app, move |a| {
        let mut g = None;
        browser::edit_tabs(a, |t| g = t.create_group(tab, title));
        g
    })
}

#[tauri::command]
async fn group_rename(app: AppHandle, id: GroupId, title: String) -> Result<(), String> {
    on_main(&app, move |a| browser::edit_tabs(a, |t| {
        t.rename_group(id, title);
    }))
}

#[tauri::command]
async fn group_set_collapsed(app: AppHandle, id: GroupId, collapsed: bool) -> Result<(), String> {
    on_main(&app, move |a| browser::edit_tabs(a, |t| {
        t.set_group_collapsed(id, collapsed);
    }))
}

#[tauri::command]
async fn group_ungroup(app: AppHandle, id: GroupId) -> Result<(), String> {
    on_main(&app, move |a| browser::edit_tabs(a, |t| t.ungroup(id)))
}

/// 탭 오른쪽 클릭 메뉴의 "항상 유지" (spec 7장): 재우거나 버리지 않는다
#[tauri::command]
async fn tab_set_keep_alive(app: AppHandle, id: TabId, keep: bool) -> Result<(), String> {
    on_main(&app, move |a| browser::set_keep_alive(a, id, keep))
}

#[tauri::command]
async fn tab_ungroup(app: AppHandle, id: TabId) -> Result<(), String> {
    on_main(&app, move |a| browser::edit_tabs(a, |t| t.remove_from_group(id)))
}

// ───────────── 북마크 (spec 5.3) ─────────────

#[tauri::command]
async fn bookmarks_get(app: AppHandle) -> Bookmarks {
    library::get(&app)
}

#[tauri::command]
async fn take_notices(app: AppHandle) -> Vec<String> {
    library::take_notices(&app)
}

#[tauri::command]
async fn bookmark_add(
    app: AppHandle,
    parent: Option<String>,
    before: Option<String>,
    title: String,
    url: String,
    icon: Option<String>,
) -> Result<String, String> {
    library::edit(&app, |b| b.add_link(parent.as_deref(), before.as_deref(), title, url, icon))
}

/// 주소창의 별 버튼: 이 주소가 북마크에 있으면 빼고, 없으면 맨 위 목록 끝에 넣는다 (spec 5.2)
#[tauri::command]
async fn bookmark_toggle(app: AppHandle, title: String, url: String, icon: Option<String>) -> Result<bool, String> {
    library::edit(&app, |b| match b.find_url(&url) {
        Some(id) => b.remove(&id).map(|_| false),
        None => b.add_link(None, None, title, url, icon).map(|_| true),
    })
}

#[tauri::command]
async fn bookmark_add_folder(app: AppHandle, parent: Option<String>, before: Option<String>, title: String) -> Result<String, String> {
    library::edit(&app, |b| b.add_folder(parent.as_deref(), before.as_deref(), title))
}

#[tauri::command]
async fn bookmark_rename(app: AppHandle, id: String, title: String) -> Result<(), String> {
    library::edit(&app, |b| b.rename(&id, title))
}

#[tauri::command]
async fn bookmark_remove(app: AppHandle, id: String) -> Result<(), String> {
    library::edit(&app, |b| b.remove(&id).map(|_| ()))
}

#[tauri::command]
async fn bookmark_move(app: AppHandle, id: String, parent: Option<String>, before: Option<String>) -> Result<(), String> {
    library::edit(&app, |b| b.move_node(&id, parent.as_deref(), before.as_deref()))
}

#[tauri::command]
async fn bookmark_set_open(app: AppHandle, id: String, open: bool) -> Result<(), String> {
    library::edit(&app, |b| b.set_open(&id, open))
}

#[tauri::command]
async fn bookmarks_set_collapsed(app: AppHandle, collapsed: bool) -> Result<(), String> {
    library::edit(&app, |b| {
        b.collapsed = collapsed;
        Ok(())
    })
}

// ───────────── 설정, 방문 기록, 다운로드 (spec 5.4) ─────────────

#[tauri::command]
async fn settings_get(app: AppHandle) -> Settings {
    settings::get(&app)
}

#[tauri::command]
async fn settings_set(app: AppHandle, value: Settings) -> Result<Settings, String> {
    settings::set(&app, value)
}

/// 다운로드 폴더 고르기. 고르지 않으면 None.
#[tauri::command]
async fn pick_download_dir(app: AppHandle) -> Result<Option<String>, String> {
    let start = settings::get(&app).download_dir();
    on_main(&app, move |a| {
        let owner = a.get_window(MAIN_WINDOW).and_then(|w| w.hwnd().ok()).map(|h| h.0 as isize);
        settings::pick_folder(owner, &start)
    })
}

#[tauri::command]
async fn history_search(app: AppHandle, query: String, before: Option<i64>) -> Result<Vec<Visit>, String> {
    app.state::<History>().search(&query, 100, before)
}

#[tauri::command]
async fn history_delete(app: AppHandle, id: i64) -> Result<(), String> {
    app.state::<History>().delete(id)
}

#[tauri::command]
async fn history_clear(app: AppHandle) -> Result<(), String> {
    app.state::<History>().clear()
}

/// 주소창 자동완성: 북마크와 방문 기록에서 최대 6개 (spec 5.2)
#[tauri::command]
async fn suggest(app: AppHandle, query: String) -> Result<Vec<Suggestion>, String> {
    const MAX: usize = 6;
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let mut out: Vec<Suggestion> = Vec::new();
    fn walk(items: &[bookmarks::Node], q: &str, out: &mut Vec<Suggestion>) {
        for n in items {
            match n {
                bookmarks::Node::Link { title, url, .. } if title.to_lowercase().contains(q) || url.to_lowercase().contains(q) => {
                    out.push(Suggestion { url: url.clone(), title: title.clone(), kind: "bookmark" })
                }
                bookmarks::Node::Folder { items, .. } => walk(items, q, out),
                _ => {}
            }
        }
    }
    walk(&library::get(&app).items, &q, &mut out);
    out.truncate(MAX);
    for h in app.state::<History>().suggest(&q, MAX as u32 * 2)? {
        if out.len() >= MAX {
            break;
        }
        if !out.iter().any(|x| x.url == h.url) {
            out.push(h);
        }
    }
    Ok(out)
}

#[tauri::command]
async fn downloads_get(app: AppHandle) -> Vec<Download> {
    downloads::list(&app)
}

#[tauri::command]
async fn download_open(app: AppHandle, id: i64) -> Result<(), String> {
    downloads::open_file(&app, id)
}

#[tauri::command]
async fn download_show(app: AppHandle, id: i64) -> Result<(), String> {
    downloads::show_in_folder(&app, id)
}

#[tauri::command]
async fn download_cancel(app: AppHandle, id: i64) -> Result<(), String> {
    on_main(&app, move |a| downloads::cancel(a, id))
}

// ───────────── 단축키, 팝업 ─────────────

/// UI에 포커스가 있을 때 누른 단축키 (spec 5.6)
#[tauri::command]
async fn run_shortcut(app: AppHandle, name: String) -> Result<(), String> {
    on_main(&app, move |a| shortcuts::run(a, &name))
}

/// 콘텐츠 위에 팝업을 띄운다. 자리는 메인 창 안쪽 좌표(논리 px).
#[tauri::command]
async fn popup_show(app: AppHandle, kind: String, x: f64, y: f64, w: f64, h: f64, data: serde_json::Value) -> Result<(), String> {
    on_main(&app, move |a| popup::show(a, &kind, x, y, w, h, data))
}

#[tauri::command]
async fn popup_hide(app: AppHandle) -> Result<(), String> {
    on_main(&app, |a| popup::hide(a))
}

/// 팝업에서 주소를 골랐다 (자동완성, 접힌 사이드바의 북마크 목록). 팝업 웹뷰가 부른다.
/// 팝업을 닫고 지금 탭을 그 주소로 옮긴다. `new_tab`이면 새 탭에서 연다.
/// 팝업을 누르면 주소창이 포커스를 잃어 UI의 목록이 먼저 비워질 수 있어서, 고른 주소를 직접 받는다.
#[tauri::command]
async fn popup_pick(app: AppHandle, url: String, new_tab: Option<bool>) -> Result<(), String> {
    let target = input::resolve(&url, settings::get(&app).search_engine).ok_or("고른 주소가 비어 있습니다.")?;
    let _ = app.emit_to(EventTarget::webview(UI_WEBVIEW), "popup_picked", &url);
    on_main(&app, move |a| {
        popup::hide(a);
        if new_tab.unwrap_or(false) {
            return browser::open_in_new_tab(a, target);
        }
        match browser::snapshot(a).active {
            Some(id) => browser::navigate(a, id, target),
            None => Ok(()),
        }
    })?
}

/// 팝업 화면이 잰 내용 높이에 팝업 창을 맞춘다 (확장 패널, 설치 확인)
#[tauri::command]
async fn popup_fit(app: AppHandle, h: f64) -> Result<(), String> {
    on_main(&app, move |a| popup::fit(a, h))
}

/// 팝업으로 띄운 메뉴에서 항목을 골랐다 (접힌 사이드바). 무엇을 할지는 UI가 안다.
#[tauri::command]
async fn popup_menu_pick(app: AppHandle, index: usize) -> Result<(), String> {
    let _ = app.emit_to(EventTarget::webview(UI_WEBVIEW), "popup_menu_picked", index);
    on_main(&app, |a| popup::hide(a))
}

// ───────────── 9단계: 크롬 확장 (spec 6.6) ─────────────

/// 시간이 걸리는 확장 작업은 WebView2 응답을 기다리므로 일꾼 스레드에서 한다
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn ext_list(app: AppHandle) -> Vec<extensions::Info> {
    extensions::list(&app)
}

/// "설치…": zip(`folder`가 거짓)이나 폴더를 골라 풀고 정보를 돌려준다. 고르지 않으면 null.
/// UI가 설치 확인 팝업(시안 M)을 띄우고, 답을 `ext_install_confirm`으로 보낸다.
#[tauri::command]
async fn ext_install_pick(app: AppHandle, folder: bool) -> Result<Option<extensions::Info>, String> {
    let picked = on_main(&app, move |a| {
        let owner = a.get_window(MAIN_WINDOW).and_then(|w| w.hwnd().ok()).map(|h| h.0 as isize);
        extensions::pick(owner, folder)
    })?;
    let Some(path) = picked else { return Ok(None) };
    let app2 = app.clone();
    let info = blocking(move || extensions::prepare(&app2, &path)).await?;
    // 접힌 사이드바의 팝업에서 눌렀어도 설치 확인은 UI가 띄운다
    let _ = app.emit_to(EventTarget::webview(UI_WEBVIEW), "install_ready", &info);
    Ok(Some(info))
}

#[tauri::command]
async fn ext_install_confirm(app: AppHandle, accept: bool) -> Result<(), String> {
    // 창을 닫기 전에 꺼내 둔다. 닫힐 때 UI가 보내는 "취소"가 먼저 꺼내 가지 않게
    let pending = extensions::take_pending(&app);
    let _ = on_main(&app, |a| popup::hide(a));
    let app2 = app.clone();
    let r = blocking(move || extensions::confirm(&app2, pending, accept)).await;
    if let Err(e) = &r {
        // 팝업은 이미 닫혔으므로 UI의 알림 자리에 보인다
        let _ = app.emit_to(EventTarget::webview(UI_WEBVIEW), "notice", e);
    }
    r
}

#[tauri::command]
async fn ext_set_enabled(app: AppHandle, id: String, enabled: bool) -> Result<(), String> {
    blocking(move || extensions::set_enabled(&app, &id, enabled)).await
}

#[tauri::command]
async fn ext_set_pinned(app: AppHandle, id: String, pinned: bool) -> Result<(), String> {
    blocking(move || extensions::set_pinned(&app, &id, pinned)).await
}

#[tauri::command]
async fn ext_remove(app: AppHandle, id: String) -> Result<(), String> {
    blocking(move || extensions::remove(&app, &id)).await
}

/// 확장의 팝업을 띄운다. (x, bottom)은 메인 창 안쪽 좌표로 팝업의 왼쪽 아래 자리
#[tauri::command]
async fn ext_open_popup(app: AppHandle, id: String, x: f64, bottom: f64) -> Result<(), String> {
    let page = extensions::page_of(&app, &id, false)?;
    let url = extensions::page_url(&id, &page)?;
    on_main(&app, move |a| popup::show_extension(a, url, x, bottom))?
}

/// 확장의 설정 페이지를 새 탭으로 연다
#[tauri::command]
async fn ext_open_options(app: AppHandle, id: String) -> Result<(), String> {
    let page = extensions::page_of(&app, &id, true)?;
    let url = extensions::page_url(&id, &page)?;
    on_main(&app, move |a| {
        popup::hide(a);
        browser::open_in_new_tab(a, url)
    })?
}

// ───────────── 8단계: 업데이트, 오류 로그 ─────────────

/// 확인해 둔 새 버전 (spec 5.7). 없으면 null
#[tauri::command]
async fn update_get(app: AppHandle) -> Option<String> {
    updater::available(&app)
}

/// "업데이트 설치": 내려받아 설치하고 다시 시작한다
#[tauri::command]
async fn update_install(app: AppHandle) -> Result<(), String> {
    updater::install(app).await
}

/// UI에서 난 오류도 로그 파일에 남긴다
#[tauri::command]
async fn log_ui(message: String) {
    log::warn(format!("UI: {}", message.chars().take(2000).collect::<String>()));
}

// ───────────── 7단계: 접힌 사이드바, 새 탭 화면, 테마, 권한 ─────────────

#[tauri::command]
async fn sidebar_get(app: AppHandle) -> bool {
    browser::sidebar_collapsed(&app)
}

#[tauri::command]
async fn sidebar_set_collapsed(app: AppHandle, collapsed: bool) -> Result<(), String> {
    on_main(&app, move |a| browser::set_sidebar_collapsed(a, collapsed))
}

/// 새 탭 화면의 자주 가는 곳 (spec 4.7): 4×2
#[tauri::command]
async fn top_sites(app: AppHandle) -> Result<Vec<TopSite>, String> {
    app.state::<History>().top_sites(8)
}

#[tauri::command]
async fn permission_requests(app: AppHandle) -> Result<Vec<permissions::Request>, String> {
    on_main(&app, |_| permissions::list())
}

/// 권한 요청에 답한다. `remember`면 사이트별로 기억한다 (spec 5.5)
#[tauri::command]
async fn permission_answer(app: AppHandle, tab: TabId, site: String, allow: bool, remember: bool) -> Result<(), String> {
    on_main(&app, move |a| permissions::answer(a, tab, &site, allow, remember))?
}

/// UI가 테마를 바꿨다. 페이지의 prefers-color-scheme과 웹뷰 빈 바탕을 맞춘다.
#[tauri::command]
async fn apply_theme(app: AppHandle, dark: bool, system: bool, background: [u8; 3]) -> Result<(), String> {
    on_main(&app, move |a| browser::apply_theme(a, dark, system, (background[0], background[1], background[2])))
}

#[tauri::command]
async fn fullscreen_state(app: AppHandle) -> bool {
    browser::is_fullscreen(&app)
}

// ───────────── 사이트 스크립트 (spec 6장) ─────────────

#[tauri::command]
async fn scripts_list(app: AppHandle) -> Vec<scripts::ScriptInfo> {
    site_scripts::list(&app)
}

/// 켜고 끈다. script.json에 저장하고, 열린 탭의 등록을 새로 한다 (새로고침 뒤 반영).
#[tauri::command]
async fn script_set_enabled(app: AppHandle, id: String, enabled: bool) -> Result<(), String> {
    let (dir, has_overrides) = script_dir(&app, &id)?;
    scripts::set_flag(&dir, "enabled", enabled)?;
    on_main(&app, move |a| {
        site_scripts::reload(a);
        browser::reapply_scripts(a, enabled && has_overrides);
    })
}

/// 사이드바 맨 아래 줄에 아이콘을 고정할지
#[tauri::command]
async fn script_set_pinned(app: AppHandle, id: String, pinned: bool) -> Result<(), String> {
    let (dir, _) = script_dir(&app, &id)?;
    scripts::set_flag(&dir, "pinned", pinned)?;
    on_main(&app, |a| site_scripts::reload(a))
}

/// "다시 불러오기": 폴더를 다시 읽고 열린 탭에 새로 등록한다
#[tauri::command]
async fn scripts_reload(app: AppHandle) -> Result<Vec<scripts::ScriptInfo>, String> {
    on_main(&app, |a| {
        site_scripts::reload(a);
        browser::reapply_scripts(a, true);
        site_scripts::list(a)
    })
}

/// "스크립트 폴더 열기"
#[tauri::command]
async fn scripts_open_folder() -> Result<(), String> {
    let dir = site_scripts::dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("스크립트 폴더를 만들지 못했습니다({e})."))?;
    std::process::Command::new("explorer.exe").arg(&dir).spawn().map(|_| ()).map_err(|e| format!("폴더를 열지 못했습니다({e})."))
}

fn script_dir(app: &AppHandle, id: &str) -> Result<(std::path::PathBuf, bool), String> {
    let store = app.state::<site_scripts::ScriptStore>();
    let all = store.0.lock().unwrap();
    all.iter()
        .find(|s| s.manifest.id == id)
        .map(|s| (s.dir.clone(), !s.manifest.resource_overrides.is_empty()))
        .ok_or_else(|| "스크립트를 찾지 못했습니다. \"다시 불러오기\"를 눌러 주세요.".to_string())
}

fn main() {
    log::install_panic_hook();
    // WebView2 런타임이 없으면 알리고 끝낸다 (spec 5.7. 설치 파일이 깔지만 지워졌을 수 있다)
    if !log::check_webview2() {
        return;
    }
    let built = tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updater::UpdateState::default())
        .manage(extensions::ExtStore::default())
        .manage(Browser::default())
        .manage(library::Library::default())
        .manage(SettingsStore::default())
        .manage(site_scripts::ScriptStore::default())
        .invoke_handler(tauri::generate_handler![
            get_state,
            tab_create,
            tab_close,
            tab_activate,
            navigate,
            go_back,
            go_forward,
            reload,
            stop,
            set_content_bounds,
            set_max_button_bounds,
            tab_open_url,
            tab_move,
            group_create,
            group_rename,
            group_set_collapsed,
            group_ungroup,
            tab_ungroup,
            tab_set_keep_alive,
            bookmarks_get,
            take_notices,
            bookmark_add,
            bookmark_toggle,
            bookmark_add_folder,
            bookmark_rename,
            bookmark_remove,
            bookmark_move,
            bookmark_set_open,
            bookmarks_set_collapsed,
            settings_get,
            settings_set,
            pick_download_dir,
            history_search,
            history_delete,
            history_clear,
            suggest,
            downloads_get,
            download_open,
            download_show,
            download_cancel,
            run_shortcut,
            popup_show,
            popup_hide,
            popup_pick,
            fullscreen_state,
            scripts_list,
            script_set_enabled,
            script_set_pinned,
            scripts_reload,
            scripts_open_folder,
            sidebar_get,
            sidebar_set_collapsed,
            top_sites,
            permission_requests,
            permission_answer,
            apply_theme,
            popup_menu_pick,
            update_get,
            update_install,
            log_ui,
            ext_list,
            ext_install_pick,
            ext_install_confirm,
            ext_set_enabled,
            ext_set_pinned,
            ext_remove,
            ext_open_popup,
            ext_open_options,
            popup_fit
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            library::load(&handle);
            if let Some(n) = settings::load(&handle) {
                library::notice(&handle, n);
            }
            let (hist, note) = History::open(&paths::state_dir().join("history.sqlite"));
            hist.mark_unfinished_failed();
            app.manage(hist);
            site_scripts::seed_examples();
            site_scripts::reload(&handle);
            if let Some(n) = note {
                library::notice(&handle, n);
            }
            // 숨긴 채로 만들고 웹뷰를 다 얹은 뒤에 보인다
            let win = WindowBuilder::new(app, MAIN_WINDOW)
                .title(APP_NAME)
                .inner_size(1280.0, 800.0)
                .min_inner_size(640.0, 400.0)
                .decorations(false)
                .visible(false)
                .focused(!dev::quiet())
                .build()?;
            let size = win.inner_size()?.to_logical::<f64>(win.scale_factor()?);
            let ui = win.add_child(
                WebviewBuilder::new(UI_WEBVIEW, WebviewUrl::App("index.html".into()))
                    .data_directory(paths::webview_dir())
                    .browser_extensions_enabled(true)
                    .focused(!dev::quiet())
                    .auto_resize(),
                LogicalPosition::new(0.0, 0.0),
                LogicalSize::new(size.width, size.height),
            )?;
            // 콘텐츠 웹뷰가 같이 쓸 환경을 UI 웹뷰에서 받아 둔다 (spec 3장 배치)
            ui.with_webview(|p| browser::remember_environment(p.environment()))?;
            frame::install(&handle, &win);
            {
                // 최대화 상태가 바뀌면 UI가 최대화·복원 아이콘을 바꾼다
                let h = handle.clone();
                let w2 = win.clone();
                win.on_window_event(move |e| {
                    // 메인 창을 옮기거나 크기를 바꾸기 시작하면 팝업을 닫는다 (spec 3장 배치).
                    // 포커스를 잃을 때는 닫지 않는다: 팝업을 누르는 순간에도 포커스가 빠져 고르기가 끊긴다.
                    // 다른 창으로 가면 주소창이 포커스를 잃으면서 UI가 닫는다.
                    if matches!(e, tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_)) {
                        let h2 = h.clone();
                        let _ = h.run_on_main_thread(move || popup::hide(&h2));
                    }
                    if let tauri::WindowEvent::Resized(_) = e {
                        let _ = h.emit_to(EventTarget::webview(UI_WEBVIEW), "window_maximized", w2.is_maximized().unwrap_or(false));
                    }
                });
            }
            popup::create(&handle, &win)?;
            extensions::start(&handle);
            // 이전 탭 복원 (설정에서 끌 수 있다). 복원하지 않으면 새 탭 하나로 시작한다.
            let restored = if settings::get(&handle).restore_tabs {
                match session::load() {
                    Ok(Some(s)) if !s.tabs.is_empty() => {
                        browser::restore(&handle, s);
                        true
                    }
                    Ok(_) => false,
                    Err(n) => {
                        library::notice(&handle, n);
                        false
                    }
                }
            } else {
                false
            };
            if !restored {
                browser::new_tab(&handle, 0, true);
            }
            session::start(&handle);
            memory::start(&handle);
            updater::start(&handle);
            dev::show_main_window(&win);
            dev::start_hook(&handle);
            Ok(())
        })
        .build(tauri::generate_context!());
    let app = match built {
        Ok(a) => a,
        Err(e) => {
            log::fatal_box(&format!(
                "TB's Browser를 시작하지 못했습니다({e}).

프로그램을 다시 켜 보고, 그래도 안 되면 {} 폴더의 tbs.log를 보내 주세요.",
                log::dir().display()
            ));
            return;
        }
    };
    app.run(|app, event| {
        // 끝낼 때 세션을 한 번 더 저장한다 (spec 8장)
        if let tauri::RunEvent::Exit = event {
            session::save_now(app);
        }
    });
}
