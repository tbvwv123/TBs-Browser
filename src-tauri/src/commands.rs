//! UI → 호스트 명령. UI 웹뷰와 팝업 창에만 허용한다 (capabilities/ui.json, popup.json).
//! 명령을 더하면 `build.rs`의 목록, capabilities, `main.rs`의 `generate_handler!`에도 넣는다.
//! 빠뜨리면 아래 `tests`가 이 목록들과 `src/lib/api.ts`의 `invoke`를 맞춰 보고 이름을 대며 실패한다.
//!
//! 명령은 UI 웹뷰의 WebView2 이벤트 처리기 안에서 불린다. 그 안에서 웹뷰를 만들면 메인 스레드가 멈추므로,
//! 웹뷰를 건드리는 명령은 async로 두고 실제 일은 `on_main`으로 메인 스레드 이벤트 루프에 넘긴다.

use tauri::{AppHandle, Manager};

use crate::{
    adblock,
    bookmarks::{self, Bookmarks},
    browser::{self, MAIN_WINDOW},
    downloads, extensions, frame,
    history::{Download, History, Suggestion, TopSite, Visit},
    input, library, log, permissions, popup, scripts,
    settings::{self, Settings},
    shortcuts, site_scripts,
    state::{GroupId, Snapshot, TabId},
    store, updater,
};

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
pub fn get_state(app: AppHandle) -> Snapshot {
    browser::snapshot(&app)
}

#[tauri::command]
pub async fn tab_create(app: AppHandle) -> Result<TabId, String> {
    // 새 탭은 목록 맨 위에 쌓인다 (spec 5.1)
    on_main(&app, |a| browser::new_tab(a, 0, true))
}

#[tauri::command]
pub async fn tab_close(app: AppHandle, id: TabId) -> Result<(), String> {
    on_main(&app, move |a| browser::close(a, id))
}

#[tauri::command]
pub async fn tab_activate(app: AppHandle, id: TabId) -> Result<(), String> {
    on_main(&app, move |a| browser::activate(a, id))
}

#[tauri::command]
pub async fn navigate(app: AppHandle, id: TabId, input: String) -> Result<(), String> {
    let url = input::resolve(&input, settings::get(&app).search_engine).ok_or("주소나 검색어를 입력해 주세요.")?;
    on_main(&app, move |a| browser::navigate(a, id, url))?
}

#[tauri::command]
pub async fn go_back(app: AppHandle, id: TabId) -> Result<(), String> {
    on_main(&app, move |a| browser::go_back(a, id))
}

#[tauri::command]
pub async fn go_forward(app: AppHandle, id: TabId) -> Result<(), String> {
    on_main(&app, move |a| browser::go_forward(a, id))
}

#[tauri::command]
pub async fn reload(app: AppHandle, id: TabId) -> Result<(), String> {
    on_main(&app, move |a| browser::reload(a, id))
}

#[tauri::command]
pub async fn stop(app: AppHandle, id: TabId) -> Result<(), String> {
    on_main(&app, move |_| browser::stop(id))
}

/// 페이지 카드 사각형과 모서리 반지름(tokens.css의 --card-radius)을 UI가 알린다.
#[tauri::command]
pub async fn set_content_bounds(app: AppHandle, x: f64, y: f64, w: f64, h: f64, radius: f64) -> Result<(), String> {
    on_main(&app, move |a| browser::set_card(a, x, y, w, h, radius))
}

/// 최대화 버튼 자리를 UI가 알린다. 그 자리에 스냅 레이아웃용 덮개 창을 놓는다.
#[tauri::command]
pub async fn set_max_button_bounds(app: AppHandle, x: f64, y: f64, w: f64, h: f64) -> Result<(), String> {
    on_main(&app, move |a| {
        if let Some(win) = a.get_window(MAIN_WINDOW) {
            frame::place_max_button(&win, x, y, w, h);
        }
    })
}

// ───────────── 탭 그룹, 탭 옮기기 (spec 5.1) ─────────────

/// 주소나 검색어를 새 탭(목록 맨 위)에서 연다. 북마크를 Ctrl+클릭하거나 가운데 클릭할 때 쓴다.
#[tauri::command]
pub async fn tab_open_url(app: AppHandle, input: String) -> Result<(), String> {
    let url = input::resolve(&input, settings::get(&app).search_engine).ok_or("열 주소가 비어 있습니다.")?;
    on_main(&app, move |a| browser::open_in_new_tab(a, url))?
}

/// 탭을 `before` 탭 앞(없으면 맨 끝)으로 옮기고 `group`에 넣는다.
#[tauri::command]
pub async fn tab_move(app: AppHandle, id: TabId, before: Option<TabId>, group: Option<GroupId>) -> Result<(), String> {
    on_main(&app, move |a| browser::edit_tabs(a, |t| {
        t.move_tab(id, before, group);
    }))
}

#[tauri::command]
pub async fn group_create(app: AppHandle, tab: TabId, title: String) -> Result<Option<GroupId>, String> {
    on_main(&app, move |a| {
        let mut g = None;
        browser::edit_tabs(a, |t| g = t.create_group(tab, title));
        g
    })
}

#[tauri::command]
pub async fn group_rename(app: AppHandle, id: GroupId, title: String) -> Result<(), String> {
    on_main(&app, move |a| browser::edit_tabs(a, |t| {
        t.rename_group(id, title);
    }))
}

#[tauri::command]
pub async fn group_set_collapsed(app: AppHandle, id: GroupId, collapsed: bool) -> Result<(), String> {
    on_main(&app, move |a| browser::edit_tabs(a, |t| {
        t.set_group_collapsed(id, collapsed);
    }))
}

#[tauri::command]
pub async fn group_ungroup(app: AppHandle, id: GroupId) -> Result<(), String> {
    on_main(&app, move |a| browser::edit_tabs(a, |t| t.ungroup(id)))
}

/// 탭 오른쪽 클릭 메뉴의 "항상 유지" (spec 7장): 재우거나 버리지 않는다
#[tauri::command]
pub async fn tab_set_keep_alive(app: AppHandle, id: TabId, keep: bool) -> Result<(), String> {
    on_main(&app, move |a| browser::set_keep_alive(a, id, keep))
}

#[tauri::command]
pub async fn tab_ungroup(app: AppHandle, id: TabId) -> Result<(), String> {
    on_main(&app, move |a| browser::edit_tabs(a, |t| t.remove_from_group(id)))
}

// ───────────── 북마크 (spec 5.3) ─────────────

#[tauri::command]
pub async fn bookmarks_get(app: AppHandle) -> Bookmarks {
    library::get(&app)
}

#[tauri::command]
pub async fn take_notices(app: AppHandle) -> Vec<String> {
    library::take_notices(&app)
}

#[tauri::command]
pub async fn bookmark_add(
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
pub async fn bookmark_toggle(app: AppHandle, title: String, url: String, icon: Option<String>) -> Result<bool, String> {
    library::edit(&app, |b| match b.find_url(&url) {
        Some(id) => b.remove(&id).map(|_| false),
        None => b.add_link(None, None, title, url, icon).map(|_| true),
    })
}

#[tauri::command]
pub async fn bookmark_add_folder(app: AppHandle, parent: Option<String>, before: Option<String>, title: String) -> Result<String, String> {
    library::edit(&app, |b| b.add_folder(parent.as_deref(), before.as_deref(), title))
}

#[tauri::command]
pub async fn bookmark_rename(app: AppHandle, id: String, title: String) -> Result<(), String> {
    library::edit(&app, |b| b.rename(&id, title))
}

#[tauri::command]
pub async fn bookmark_remove(app: AppHandle, id: String) -> Result<(), String> {
    library::edit(&app, |b| b.remove(&id).map(|_| ()))
}

#[tauri::command]
pub async fn bookmark_move(app: AppHandle, id: String, parent: Option<String>, before: Option<String>) -> Result<(), String> {
    library::edit(&app, |b| b.move_node(&id, parent.as_deref(), before.as_deref()))
}

#[tauri::command]
pub async fn bookmark_set_open(app: AppHandle, id: String, open: bool) -> Result<(), String> {
    library::edit(&app, |b| b.set_open(&id, open))
}

#[tauri::command]
pub async fn bookmarks_set_collapsed(app: AppHandle, collapsed: bool) -> Result<(), String> {
    library::edit(&app, |b| {
        b.collapsed = collapsed;
        Ok(())
    })
}

// ───────────── 설정, 방문 기록, 다운로드 (spec 5.4) ─────────────

#[tauri::command]
pub async fn settings_get(app: AppHandle) -> Settings {
    settings::get(&app)
}

#[tauri::command]
pub async fn settings_set(app: AppHandle, value: Settings) -> Result<Settings, String> {
    let lists_before = settings::get(&app).adblock.lists;
    let s = settings::set(&app, value)?;
    // 광고 차단 설정을 반영한다 (켜짐·끈 사이트는 바로, 목록을 바꾸면 엔진을 다시 만든다)
    let lists_changed = lists_before != s.adblock.lists;
    on_main(&app, move |a| {
        adblock::apply_settings(a);
        if lists_changed {
            adblock::lists_changed(a);
        }
    })?;
    Ok(s)
}

// ───────────── 10단계: 광고 차단 (spec 6.7) ─────────────

/// 지금 탭의 광고 차단 상태 (방패 버튼, 광고 차단 팝업)
#[tauri::command]
pub async fn adblock_status(app: AppHandle, tab: Option<TabId>) -> Result<adblock::Status, String> {
    on_main(&app, move |a| adblock::status(a, tab))
}

/// 이 사이트에서 광고 차단을 켜거나 끈다
#[tauri::command]
pub async fn adblock_set_site(app: AppHandle, site: String, enabled: bool) -> Result<(), String> {
    let mut s = settings::get(&app);
    s.adblock.disabled_sites.retain(|x| x != &site);
    if !enabled {
        s.adblock.disabled_sites.push(site);
    }
    settings::set(&app, s)?;
    on_main(&app, |a| adblock::apply_settings(a))
}

#[tauri::command]
pub async fn adblock_lists(app: AppHandle) -> Vec<adblock::ListInfo> {
    adblock::lists(&app)
}

#[tauri::command]
pub async fn adblock_update_now(app: AppHandle) {
    adblock::update_now(&app);
}

/// 광고 차단 팝업의 "새로고침": 지금 탭을 다시 불러온다
#[tauri::command]
pub async fn adblock_reload(app: AppHandle) -> Result<(), String> {
    on_main(&app, |a| {
        if let Some(id) = browser::snapshot(a).active {
            browser::reload(a, id);
        }
        popup::hide(a);
    })
}

/// 다운로드 폴더 고르기. 고르지 않으면 None.
#[tauri::command]
pub async fn pick_download_dir(app: AppHandle) -> Result<Option<String>, String> {
    let start = settings::get(&app).download_dir();
    on_main(&app, move |a| settings::pick_folder(browser::main_hwnd(a), &start))
}

#[tauri::command]
pub async fn history_search(app: AppHandle, query: String, before: Option<i64>) -> Result<Vec<Visit>, String> {
    app.state::<History>().search(&query, 100, before)
}

#[tauri::command]
pub async fn history_delete(app: AppHandle, id: i64) -> Result<(), String> {
    app.state::<History>().delete(id)
}

#[tauri::command]
pub async fn history_clear(app: AppHandle) -> Result<(), String> {
    app.state::<History>().clear()
}

/// 주소창 자동완성: 북마크와 방문 기록에서 최대 6개 (spec 5.2)
#[tauri::command]
pub async fn suggest(app: AppHandle, query: String) -> Result<Vec<Suggestion>, String> {
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
pub async fn downloads_get(app: AppHandle) -> Vec<Download> {
    downloads::list(&app)
}

#[tauri::command]
pub async fn download_open(app: AppHandle, id: i64) -> Result<(), String> {
    downloads::open_file(&app, id)
}

#[tauri::command]
pub async fn download_show(app: AppHandle, id: i64) -> Result<(), String> {
    downloads::show_in_folder(&app, id)
}

#[tauri::command]
pub async fn download_cancel(app: AppHandle, id: i64) -> Result<(), String> {
    on_main(&app, move |a| downloads::cancel(a, id))
}

// ───────────── 단축키, 팝업 ─────────────

/// UI에 포커스가 있을 때 누른 단축키 (spec 5.6)
#[tauri::command]
pub async fn run_shortcut(app: AppHandle, name: String) -> Result<(), String> {
    on_main(&app, move |a| shortcuts::run(a, &name))
}

/// 콘텐츠 위에 팝업을 띄운다. 자리는 메인 창 안쪽 좌표(논리 px).
#[tauri::command]
pub async fn popup_show(app: AppHandle, kind: String, x: f64, y: f64, w: f64, h: f64, data: serde_json::Value) -> Result<(), String> {
    on_main(&app, move |a| popup::show(a, &kind, x, y, w, h, data))
}

#[tauri::command]
pub async fn popup_hide(app: AppHandle) -> Result<(), String> {
    on_main(&app, |a| popup::hide(a))
}

/// 팝업에서 주소를 골랐다 (자동완성, 접힌 사이드바의 북마크 목록). 팝업 웹뷰가 부른다.
/// 팝업을 닫고 지금 탭을 그 주소로 옮긴다. `new_tab`이면 새 탭에서 연다.
/// 팝업을 누르면 주소창이 포커스를 잃어 UI의 목록이 먼저 비워질 수 있어서, 고른 주소를 직접 받는다.
#[tauri::command]
pub async fn popup_pick(app: AppHandle, url: String, new_tab: Option<bool>) -> Result<(), String> {
    let target = input::resolve(&url, settings::get(&app).search_engine).ok_or("고른 주소가 비어 있습니다.")?;
    browser::emit_ui(&app, "popup_picked", &url);
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
pub async fn popup_fit(app: AppHandle, h: f64) -> Result<(), String> {
    on_main(&app, move |a| popup::fit(a, h))
}

/// 팝업으로 띄운 메뉴에서 항목을 골랐다 (접힌 사이드바). 무엇을 할지는 UI가 안다.
#[tauri::command]
pub async fn popup_menu_pick(app: AppHandle, index: usize) -> Result<(), String> {
    browser::emit_ui(&app, "popup_menu_picked", index);
    on_main(&app, |a| popup::hide(a))
}

// ───────────── 9단계: 크롬 확장 (spec 6.6) ─────────────

/// 기다리는 일(WebView2 응답, 내려받기)은 일꾼 스레드에서 한다
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn ext_list(app: AppHandle) -> Vec<extensions::Info> {
    extensions::list(&app)
}

/// "설치…": zip(`folder`가 거짓)이나 폴더를 골라 풀고 정보를 돌려준다. 고르지 않으면 null.
/// UI가 설치 확인 팝업(시안 M)을 띄우고, 답을 `ext_install_confirm`으로 보낸다.
#[tauri::command]
pub async fn ext_install_pick(app: AppHandle, folder: bool) -> Result<Option<extensions::Info>, String> {
    let picked = on_main(&app, move |a| extensions::pick(browser::main_hwnd(a), folder))?;
    let Some(path) = picked else { return Ok(None) };
    let app2 = app.clone();
    let info = blocking(move || extensions::prepare(&app2, &path)).await?;
    // 접힌 사이드바의 팝업에서 눌렀어도 설치 확인은 UI가 띄운다
    browser::emit_ui(&app, "install_ready", &info);
    Ok(Some(info))
}

#[tauri::command]
pub async fn ext_install_confirm(app: AppHandle, accept: bool) -> Result<(), String> {
    // 창을 닫기 전에 꺼내 둔다. 닫힐 때 UI가 보내는 "취소"가 먼저 꺼내 가지 않게
    let pending = extensions::take_pending(&app);
    let _ = on_main(&app, |a| popup::hide(a));
    let app2 = app.clone();
    let r = blocking(move || extensions::confirm(&app2, pending, accept)).await;
    if let Err(e) = &r {
        // 팝업은 이미 닫혔으므로 UI의 알림 자리에 보인다
        browser::emit_ui(&app, "notice", e);
    }
    r
}

#[tauri::command]
pub async fn ext_set_enabled(app: AppHandle, id: String, enabled: bool) -> Result<(), String> {
    blocking(move || extensions::set_enabled(&app, &id, enabled)).await
}

#[tauri::command]
pub async fn ext_set_pinned(app: AppHandle, id: String, pinned: bool) -> Result<(), String> {
    blocking(move || extensions::set_pinned(&app, &id, pinned)).await
}

#[tauri::command]
pub async fn ext_remove(app: AppHandle, id: String) -> Result<(), String> {
    blocking(move || extensions::remove(&app, &id)).await
}

/// 확장의 팝업을 띄운다. (x, bottom)은 메인 창 안쪽 좌표로 팝업의 왼쪽 아래 자리
#[tauri::command]
pub async fn ext_open_popup(app: AppHandle, id: String, x: f64, bottom: f64) -> Result<(), String> {
    let page = extensions::page_of(&app, &id, false)?;
    let url = extensions::page_url(&id, &page)?;
    let name = extensions::list(&app).into_iter().find(|i| i.id == id).map(|i| i.name).unwrap_or_default();
    on_main(&app, move |a| popup::show_extension(a, &id, &name, url, x, bottom))?
}

/// 확장의 설정 페이지를 새 탭으로 연다
#[tauri::command]
pub async fn ext_open_options(app: AppHandle, id: String) -> Result<(), String> {
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
pub async fn update_get(app: AppHandle) -> Option<String> {
    updater::available(&app)
}

/// "업데이트 설치": 내려받아 설치하고 다시 시작한다
#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<(), String> {
    updater::install(app).await
}

/// UI에서 난 오류도 로그 파일에 남긴다
#[tauri::command]
pub async fn log_ui(message: String) {
    log::warn(format!("UI: {}", message.chars().take(2000).collect::<String>()));
}

// ───────────── 7단계: 접힌 사이드바, 새 탭 화면, 테마, 권한 ─────────────

#[tauri::command]
pub async fn sidebar_get(app: AppHandle) -> bool {
    browser::sidebar_collapsed(&app)
}

#[tauri::command]
pub async fn sidebar_set_collapsed(app: AppHandle, collapsed: bool) -> Result<(), String> {
    on_main(&app, move |a| browser::set_sidebar_collapsed(a, collapsed))
}

/// 새 탭 화면의 자주 가는 곳 (spec 4.7): 4×2
#[tauri::command]
pub async fn top_sites(app: AppHandle) -> Result<Vec<TopSite>, String> {
    app.state::<History>().top_sites(8)
}

#[tauri::command]
pub async fn permission_requests(app: AppHandle) -> Result<Vec<permissions::Request>, String> {
    on_main(&app, |_| permissions::list())
}

/// 권한 요청에 답한다. `remember`면 사이트별로 기억한다 (spec 5.5)
#[tauri::command]
pub async fn permission_answer(app: AppHandle, tab: TabId, site: String, allow: bool, remember: bool) -> Result<(), String> {
    on_main(&app, move |a| permissions::answer(a, tab, &site, allow, remember))?
}

/// UI가 테마를 바꿨다. 페이지의 prefers-color-scheme과 웹뷰 빈 바탕을 맞춘다.
#[tauri::command]
pub async fn apply_theme(app: AppHandle, dark: bool, system: bool, background: [u8; 3]) -> Result<(), String> {
    on_main(&app, move |a| browser::apply_theme(a, dark, system, (background[0], background[1], background[2])))
}

#[tauri::command]
pub async fn fullscreen_state(app: AppHandle) -> bool {
    browser::is_fullscreen(&app)
}

// ───────────── 사이트 스크립트 (spec 6장) ─────────────

#[tauri::command]
pub async fn scripts_list(app: AppHandle) -> Vec<scripts::ScriptInfo> {
    site_scripts::list(&app)
}

/// 켜고 끈다. script.json에 저장하고, 열린 탭의 등록을 새로 한다 (새로고침 뒤 반영).
/// 모든 사이트에서 돌거나 새 권한(tabs, storage)이 있으면, 확인(`confirmed`) 없이는 켜지 않고 켜기 확인 창을 띄운다 (spec 6.1, 6.3).
#[tauri::command]
pub async fn script_set_enabled(app: AppHandle, id: String, enabled: bool, confirmed: Option<bool>) -> Result<(), String> {
    let (dir, has_overrides) = script_dir(&app, &id)?;
    if enabled && confirmed != Some(true) {
        let prompt = site_scripts::with_script(&app, &id, |s| s.needs_confirm().then(|| site_scripts::prompt_info(s)));
        if let Some(info) = prompt.flatten() {
            browser::emit_ui(&app, "install_ready", info);
            return Ok(());
        }
    }
    scripts::set_flag(&dir, "enabled", enabled)?;
    on_main(&app, move |a| {
        site_scripts::reload(a);
        browser::reapply_scripts(a, enabled && has_overrides);
    })
}

/// 사이드바 맨 아래 줄에 아이콘을 고정할지
#[tauri::command]
pub async fn script_set_pinned(app: AppHandle, id: String, pinned: bool) -> Result<(), String> {
    let (dir, _) = script_dir(&app, &id)?;
    scripts::set_flag(&dir, "pinned", pinned)?;
    on_main(&app, |a| site_scripts::reload(a))
}

/// 스크립트 하나 (설정 화면, spec 6.4 시안 N)
#[tauri::command]
pub async fn script_get(app: AppHandle, id: String) -> Option<scripts::ScriptInfo> {
    site_scripts::list(&app).into_iter().find(|s| s.id == id)
}

/// 설정 값 하나를 바꾼다. 열린 페이지에도 알린다 (tbs.onSettingsChanged)
#[tauri::command]
pub async fn script_set_value(app: AppHandle, id: String, key: String, value: serde_json::Value) -> Result<(), String> {
    site_scripts::with_script(&app, &id, |s| scripts::set_value(s, &key, &value)).ok_or(site_scripts::NOT_FOUND)??;
    on_main(&app, move |a| site_scripts::settings_changed(a, &id))
}

/// 설정 화면을 탭으로 연다
#[tauri::command]
pub async fn script_open_settings(app: AppHandle, id: String) -> Result<(), String> {
    script_dir(&app, &id)?;
    on_main(&app, move |a| browser::open_internal(a, &format!("tbs://scripts/{id}")))
}

/// 지금 탭에서 스크립트의 동작을 실행한다 ("이 탭에서 실행")
#[tauri::command]
pub async fn script_run_action(app: AppHandle, id: String, action: String) -> Result<(), String> {
    on_main(&app, move |a| {
        let tab = browser::snapshot(a).active.ok_or("열린 탭이 없습니다.")?;
        site_scripts::run_action(a, tab, &id, &action)
    })?
}

/// 스크립트 폴더를 휴지통으로 옮긴다 (조용히 지우지 않는다: 휴지통에서 되살릴 수 있다)
#[tauri::command]
pub async fn script_remove(app: AppHandle, id: String) -> Result<(), String> {
    let (dir, _) = script_dir(&app, &id)?;
    site_scripts::to_recycle_bin(&dir)?;
    on_main(&app, |a| {
        site_scripts::reload(a);
        browser::reapply_scripts(a, false);
    })
}

// ───────────── 스크립트 저장소 (spec 6.8) ─────────────

/// 찾아보기 목록. `refresh`면 새로 받는다
#[tauri::command]
pub async fn store_list(app: AppHandle, refresh: bool) -> Result<Vec<serde_json::Value>, String> {
    blocking(move || store::list_json(&app, refresh)).await
}

/// "설치"·"업데이트": 확인 창을 띄우거나(권한이 늘지 않은 업데이트는 바로) 설치한다
#[tauri::command]
pub async fn store_install(app: AppHandle, id: String) -> Result<(), String> {
    blocking(move || store::begin(&app, &id)).await
}

/// 설치·업데이트 확인 창의 답
#[tauri::command]
pub async fn store_confirm(app: AppHandle, id: String, accept: bool) {
    store::confirm(&app, &id, accept);
}

/// "다시 불러오기": 폴더를 다시 읽고 열린 탭에 새로 등록한다
#[tauri::command]
pub async fn scripts_reload(app: AppHandle) -> Result<Vec<scripts::ScriptInfo>, String> {
    on_main(&app, |a| {
        site_scripts::reload(a);
        browser::reapply_scripts(a, true);
        site_scripts::list(a)
    })
}

/// "스크립트 폴더 열기"
#[tauri::command]
pub async fn scripts_open_folder() -> Result<(), String> {
    let dir = site_scripts::dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("스크립트 폴더를 만들지 못했습니다({e})."))?;
    std::process::Command::new("explorer.exe").arg(&dir).spawn().map(|_| ()).map_err(|e| format!("폴더를 열지 못했습니다({e})."))
}

/// 스크립트의 (폴더, 리소스 바꿔치기가 있는지)
fn script_dir(app: &AppHandle, id: &str) -> Result<(std::path::PathBuf, bool), String> {
    site_scripts::with_script(app, id, |s| (s.dir.clone(), !s.manifest.resource_overrides.is_empty())).ok_or_else(|| site_scripts::NOT_FOUND.to_string())
}

/// 명령 이름을 손으로 적는 곳(`build.rs`, `main.rs`, capabilities, `api.ts`)이 이 파일과 맞는지 본다.
#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    const COMMANDS_RS: &str = include_str!("commands.rs");
    const MAIN_RS: &str = include_str!("main.rs");
    const BUILD_RS: &str = include_str!("../build.rs");
    const UI_JSON: &str = include_str!("../capabilities/ui.json");
    const POPUP_JSON: &str = include_str!("../capabilities/popup.json");
    const API_TS: &str = include_str!("../../src/lib/api.ts");

    /// 겹치는 이름이 있으면 실패한다
    fn to_set(names: Vec<String>, place: &str) -> BTreeSet<String> {
        assert!(!names.is_empty(), "{place}에서 명령을 하나도 못 읽었습니다. 형식이 바뀌었으면 이 테스트의 읽는 부분을 고칩니다.");
        let mut set = BTreeSet::new();
        for n in names {
            assert!(set.insert(n.clone()), "{place}에 `{n}`이 두 번 있습니다.");
        }
        set
    }

    /// `#[tauri::command]` 바로 아래(문서 주석·다른 속성은 건너뜀)의 `pub fn`/`pub async fn` 이름
    fn defined() -> BTreeSet<String> {
        // 이 테스트 모듈부터는 읽지 않는다
        let src = COMMANDS_RS.split("\n#[cfg(test)]\nmod tests {").next().unwrap();
        let mut attrs = 0;
        let mut names = Vec::new();
        let mut lines = src.lines();
        while let Some(line) = lines.next() {
            if !line.contains("tauri::command") {
                continue;
            }
            assert_eq!(line.trim(), "#[tauri::command]", "commands.rs: 읽을 수 없는 명령 속성 `{line}`");
            attrs += 1;
            let f = lines
                .by_ref()
                .map(str::trim)
                .find(|l| !l.is_empty() && !l.starts_with("#[") && !l.starts_with("//"))
                .unwrap_or_else(|| panic!("commands.rs: 마지막 #[tauri::command] 아래에 함수가 없습니다."));
            let rest = f
                .strip_prefix("pub async fn ")
                .or_else(|| f.strip_prefix("pub fn "))
                .unwrap_or_else(|| panic!("commands.rs: #[tauri::command] 아래가 `pub fn`/`pub async fn`이 아닙니다: `{f}`"));
            let name: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            assert!(!name.is_empty(), "commands.rs: 함수 이름을 못 읽었습니다: `{f}`");
            names.push(name);
        }
        assert_eq!(attrs, names.len(), "commands.rs: #[tauri::command] 수와 읽은 함수 수가 다릅니다.");
        to_set(names, "commands.rs")
    }

    /// `start`부터 다음 `]`까지를 `,`로 나눈 항목
    fn list_after<'a>(src: &'a str, start: &str, place: &str) -> Vec<&'a str> {
        assert_eq!(src.matches(start).count(), 1, "{place}에 `{start}`가 한 번만 있어야 합니다.");
        let body = &src[src.find(start).unwrap() + start.len()..];
        let body = &body[..body.find(']').unwrap_or_else(|| panic!("{place}: `{start}` 뒤에 `]`가 없습니다."))];
        body.split(',').map(str::trim).filter(|s| !s.is_empty()).collect()
    }

    fn handlers() -> BTreeSet<String> {
        let names = list_after(MAIN_RS, "generate_handler![", "main.rs")
            .into_iter()
            .map(|s| s.strip_prefix("commands::").unwrap_or_else(|| panic!("main.rs generate_handler!: `commands::`로 시작하지 않는 항목 `{s}`")).to_string())
            .collect();
        to_set(names, "main.rs generate_handler!")
    }

    fn manifest() -> BTreeSet<String> {
        let names = list_after(BUILD_RS, ".commands(&[", "build.rs")
            .into_iter()
            .map(|s| s.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or_else(|| panic!("build.rs commands: 문자열이 아닌 항목 `{s}`")).to_string())
            .collect();
        to_set(names, "build.rs AppManifest::commands")
    }

    /// 앱 명령 권한만 명령 이름으로 바꿔 돌려준다. Tauri는 명령 `go_back`에 `allow-go-back`을 만든다
    /// (permissions/autogenerated/*.toml). `core:` 같은 플러그인 권한은 건너뛴다.
    fn allowed(json: &str, place: &str) -> BTreeSet<String> {
        let v: serde_json::Value = serde_json::from_str(json).unwrap_or_else(|e| panic!("{place}: JSON을 못 읽었습니다({e})."));
        let perms = v["permissions"].as_array().unwrap_or_else(|| panic!("{place}: `permissions` 배열이 없습니다."));
        let mut names = Vec::new();
        for p in perms {
            let id = p.as_str().or_else(|| p["identifier"].as_str()).unwrap_or_else(|| panic!("{place}: 읽을 수 없는 권한 `{p}`"));
            if id.contains(':') {
                continue;
            }
            let cmd = id.strip_prefix("allow-").unwrap_or_else(|| panic!("{place}: `allow-` 꼴이 아닌 앱 권한 `{id}`"));
            names.push(cmd.replace('-', "_"));
        }
        to_set(names, place)
    }

    /// `invoke<…>("name"` / `invoke("name"`의 name
    fn invoked() -> BTreeSet<String> {
        let mut names = Vec::new();
        for (i, _) in API_TS.match_indices("invoke") {
            let before = API_TS[..i].chars().next_back();
            if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.') {
                continue;
            }
            let rest = &API_TS[i + "invoke".len()..];
            if !rest.starts_with(['<', '(']) {
                continue; // import { invoke }
            }
            let line = rest.lines().next().unwrap_or("");
            let arg = rest[rest.find('(').unwrap() + 1..].trim_start();
            let name = arg
                .strip_prefix('"')
                .and_then(|a| a.split_once('"'))
                .map(|(n, _)| n)
                .unwrap_or_else(|| panic!("api.ts: 명령 이름이 문자열이 아닌 invoke `invoke{line}`"));
            names.push(name.to_string());
        }
        to_set(names, "api.ts invoke")
    }

    fn assert_same(want: &BTreeSet<String>, got: &BTreeSet<String>, place: &str) {
        let missing: Vec<_> = want.difference(got).collect();
        let extra: Vec<_> = got.difference(want).collect();
        assert!(missing.is_empty() && extra.is_empty(), "{place}: 빠진 명령 {missing:?}, commands.rs에 없는 명령 {extra:?}");
    }

    fn assert_subset(all: &BTreeSet<String>, got: &BTreeSet<String>, place: &str) {
        let extra: Vec<_> = got.difference(all).collect();
        assert!(extra.is_empty(), "{place}: commands.rs에 없는 명령 {extra:?}");
    }

    #[test]
    fn command_lists_match() {
        let all = defined();
        assert_same(&all, &handlers(), "main.rs generate_handler!");
        assert_same(&all, &manifest(), "build.rs AppManifest::commands");
    }

    #[test]
    fn capabilities_name_real_commands() {
        let all = defined();
        let ui = allowed(UI_JSON, "capabilities/ui.json");
        let popup = allowed(POPUP_JSON, "capabilities/popup.json");
        assert_subset(&all, &ui, "capabilities/ui.json");
        assert_subset(&all, &popup, "capabilities/popup.json");
        let none: Vec<_> = all.iter().filter(|c| !ui.contains(*c) && !popup.contains(*c)).collect();
        assert!(none.is_empty(), "어느 capability에도 허용되지 않은 명령 {none:?}");
    }

    #[test]
    fn api_invokes_every_command() {
        assert_same(&defined(), &invoked(), "api.ts invoke");
    }
}
