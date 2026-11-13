//! 크롬 확장 호환층 (spec 6.6). WebView2에 없는 두 가지를 우리가 채운다.
//!
//! 1. "지금 탭": WebView2는 웹뷰마다 "창 하나에 탭 하나"로 보여서, 확장 팝업이 활성 탭을 물으면 팝업 자신이 나온다.
//!    확장 HTML에 `tbs-compat/page.js`를 끼워, 그런 물음에는 TB's Browser가 지금 보여 주는 탭을 돌려준다.
//! 2. 오른쪽 클릭 메뉴: WebView2는 `chrome.contextMenus` 항목을 그리지 않는다. 서비스 워커 앞에 `tbs-compat/sw-shim.js`를
//!    끼워 항목을 저장소에 적게 하고, 보이지 않는 브리지 페이지(`tbs-compat/bridge.html`)가 그 목록을 알려 준다.
//!    페이지의 오른쪽 클릭 메뉴에 넣고, 누르면 브리지를 거쳐 확장의 처리기를 부른다.
//!
//! 끼우는 일은 설치할 때와 시작할 때 한다 (이미 끼웠으면 그대로 둔다). 원래 서비스 워커 이름은 `tbs-compat/original-sw.txt`에 둔다.

use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    path::Path,
    sync::Mutex,
};

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, Url, WebviewUrl};
use webview2_com::{Microsoft::Web::WebView2::Win32::*, *};
use windows::core::{Interface, HSTRING, PWSTR};

use crate::{browser, extensions, log, paths};
use std::result::Result;

const PAGE_JS: &str = include_str!("../ext-compat/page.js");
const SW_SHIM: &str = include_str!("../ext-compat/sw-shim.js");
const BRIDGE_JS: &str = include_str!("../ext-compat/bridge.js");
const BRIDGE_HTML: &str = "<!doctype html><meta charset=\"utf-8\"><title>TB's Browser</title><script src=\"bridge.js\"></script>\n";
const SW_WRAPPER: &str = "tbs-compat/sw.js";
const PAGE_TAG: &str = "<script src=\"/tbs-compat/page.js\"></script>";

// ───────────── 끼우기 ─────────────

fn write_if_changed(path: &Path, text: &str) -> Result<bool, String> {
    if fs::read_to_string(path).ok().as_deref() == Some(text) {
        return Ok(false);
    }
    fs::write(path, text).map_err(|e| format!("확장 호환 파일을 쓰지 못했습니다({e})."))?;
    Ok(true)
}

/// 확장 폴더에 호환 파일을 넣고 매니페스트와 HTML을 고친다. 바뀐 것이 있으면 참 (확장을 다시 켜야 반영된다).
pub fn patch(dir: &Path) -> Result<bool, String> {
    // 크롬은 "_"로 시작하는 파일·폴더 이름을 쓰지 못하게 한다 (있으면 확장을 불러오지 않는다)
    let _ = fs::remove_dir_all(dir.join("__tbs"));
    let tbs = dir.join("tbs-compat");
    fs::create_dir_all(&tbs).map_err(|e| format!("확장 호환 폴더를 만들지 못했습니다({e})."))?;
    let mut changed = false;
    changed |= write_if_changed(&tbs.join("page.js"), PAGE_JS)?;
    changed |= write_if_changed(&tbs.join("sw-shim.js"), SW_SHIM)?;
    changed |= write_if_changed(&tbs.join("bridge.js"), BRIDGE_JS)?;
    changed |= write_if_changed(&tbs.join("bridge.html"), BRIDGE_HTML)?;

    // 서비스 워커 앞에 sw-shim.js를 끼운다
    let manifest_path = dir.join("manifest.json");
    let raw = fs::read_to_string(&manifest_path).map_err(|e| format!("manifest.json을 읽지 못했습니다({e})."))?;
    let mut m: Value = serde_json::from_str(raw.trim_start_matches('\u{feff}')).map_err(|e| format!("manifest.json 형식이 잘못되었습니다({e})."))?;
    if let Some(bg) = m.get_mut("background").and_then(|b| b.as_object_mut()) {
        if let Some(sw) = bg.get("service_worker").and_then(|v| v.as_str()).map(|s| s.trim_start_matches('/').to_string()) {
            let original = if sw == SW_WRAPPER { fs::read_to_string(tbs.join("original-sw.txt")).unwrap_or_default() } else { sw.clone() };
            if !original.is_empty() {
                let module = bg.get("type").and_then(|v| v.as_str()) == Some("module");
                let wrapper = if module {
                    format!("import \"/tbs-compat/sw-shim.js\";\nimport \"/{original}\";\n")
                } else {
                    format!("importScripts(\"/tbs-compat/sw-shim.js\", \"/{original}\");\n")
                };
                changed |= write_if_changed(&tbs.join("original-sw.txt"), &original)?;
                changed |= write_if_changed(&dir.join(SW_WRAPPER), &wrapper)?;
                if sw != SW_WRAPPER {
                    bg.insert("service_worker".into(), Value::String(SW_WRAPPER.into()));
                    let text = serde_json::to_string_pretty(&m).map_err(|e| e.to_string())?;
                    fs::write(&manifest_path, text).map_err(|e| format!("manifest.json을 고치지 못했습니다({e})."))?;
                    changed = true;
                }
            }
        }
    }

    // 확장의 HTML 페이지 맨 앞에 page.js를 끼운다
    changed |= patch_html(dir, dir)?;
    Ok(changed)
}

fn patch_html(root: &Path, dir: &Path) -> Result<bool, String> {
    let mut changed = false;
    for e in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if dir == root && (name == "tbs-compat" || name == "_locales" || name == "_metadata") {
                continue;
            }
            changed |= patch_html(root, &path)?;
        } else if name.to_ascii_lowercase().ends_with(".html") {
            let Ok(text) = fs::read_to_string(&path) else { continue };
            if text.contains("/tbs-compat/page.js") {
                continue;
            }
            let lower = text.to_ascii_lowercase();
            let at = lower.find("<head").and_then(|i| lower[i..].find('>').map(|j| i + j + 1)).unwrap_or(0);
            let patched = format!("{}{PAGE_TAG}{}", &text[..at], &text[at..]);
            fs::write(&path, patched).map_err(|e| format!("확장 페이지를 고치지 못했습니다({e})."))?;
            changed = true;
        }
    }
    Ok(changed)
}

// ───────────── 확장 페이지가 보낸 메시지 ─────────────

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct MenuItem {
    pub id: String,
    pub title: Option<String>,
    pub contexts: Option<Vec<String>>,
    pub document_url_patterns: Option<Vec<String>>,
    pub target_url_patterns: Option<Vec<String>>,
    pub parent_id: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub checked: Option<bool>,
    pub enabled: Option<bool>,
    pub visible: Option<bool>,
}

/// 확장마다 받은 오른쪽 클릭 메뉴 항목 (확장 ID → 이름, 항목)
static MENUS: Mutex<Option<HashMap<String, (String, Vec<MenuItem>)>>> = Mutex::new(None);

fn ext_of(source: &str) -> Option<&str> {
    source.strip_prefix("chrome-extension://")?.split('/').next()
}

/// 크롬 주소 패턴이 주소에 맞는지 ("https://*.example.com/*", "<all_urls>").
/// UI의 chromeMatches와 함께 tests/rules-vectors.json(`chromePatterns`)으로 시험한다
pub fn chrome_matches(pattern: &str, url: &str) -> bool {
    let Ok(u) = Url::parse(url) else { return false };
    if u.scheme() != "http" && u.scheme() != "https" {
        return false;
    }
    if pattern == "<all_urls>" {
        return true;
    }
    let Some((scheme, rest)) = pattern.split_once("://") else { return false };
    if scheme != "*" && scheme != u.scheme() {
        return false;
    }
    let (host, path) = rest.split_once('/').map(|(h, p)| (h, format!("/{p}"))).unwrap_or((rest, "/*".into()));
    let uh = u.host_str().unwrap_or("");
    let host_ok = host == "*" || match host.strip_prefix("*.") {
        Some(base) => uh == base || uh.ends_with(&format!(".{base}")),
        None => uh == host,
    };
    let target = format!("{}{}", u.path(), u.query().map(|q| format!("?{q}")).unwrap_or_default());
    host_ok && crate::scripts::pattern_matches(&path, &target)
}

/// 확장 페이지가 보낸 호환 메시지면 처리하고 참을 돌려준다
pub fn handle_message(app: &AppHandle, core: &ICoreWebView2, source: &str, text: &str) -> bool {
    let Some(ext) = ext_of(source) else { return false };
    let Ok(msg) = serde_json::from_str::<Value>(text) else { return false };
    match msg.get("tbs").and_then(|v| v.as_str()) {
        Some("activeTab") => {
            let id = msg.get("id").cloned().unwrap_or(Value::Null);
            let snap = browser::snapshot(app);
            let tab = snap.active.and_then(|a| snap.tabs.into_iter().find(|t| t.id == a));
            // 확장이 원래 볼 수 있는 주소만 알려 준다 ("tabs" 권한이나 그 사이트 권한이 있을 때)
            let info = extensions::list(app).into_iter().find(|i| i.id == ext);
            let allowed = |url: &str| info.as_ref().is_some_and(|i| i.tabs_permission || i.all_sites || i.matches.iter().any(|p| chrome_matches(p, url)));
            let reply = match tab.filter(|t| allowed(&t.url)) {
                Some(t) => json!({ "tbsReply": id, "tab": { "url": t.url, "title": t.title } }),
                None => json!({ "tbsReply": id, "tab": null }),
            };
            unsafe {
                let _ = core.PostWebMessageAsJson(&HSTRING::from(reply.to_string()));
            }
            true
        }
        Some("openTab") => {
            // 확장이 새 탭을 열었다 (chrome.tabs.create, openOptionsPage). 웹 주소나 그 확장 자신의 페이지만 연다
            let url = msg.get("url").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let ok = Url::parse(&url).ok().filter(|u| {
                matches!(u.scheme(), "http" | "https") || (u.scheme() == "chrome-extension" && u.host_str() == Some(ext))
            });
            if let Some(u) = ok {
                browser::later(app, move |a| {
                    let _ = browser::open_in_new_tab(a, u);
                });
            }
            true
        }
        Some("menus") => {
            // 브리지 페이지가 보낸 것만 믿는다
            if source != format!("chrome-extension://{ext}/tbs-compat/bridge.html") {
                return true;
            }
            let items: Vec<MenuItem> = serde_json::from_value(msg.get("items").cloned().unwrap_or(Value::Null)).unwrap_or_default();
            let name = extensions::list(app).into_iter().find(|i| i.id == ext).map(|i| i.name).unwrap_or_default();
            MENUS.lock().unwrap().get_or_insert_with(HashMap::new).insert(ext.to_string(), (name, items));
            true
        }
        _ => false,
    }
}

/// WebView2 웹뷰에 호환 메시지 처리기를 건다 (확장 팝업 창, 브리지)
pub fn listen(app: &AppHandle, core: &ICoreWebView2) {
    let app = app.clone();
    let mut token = 0i64;
    unsafe {
        let _ = core.add_WebMessageReceived(
            &WebMessageReceivedEventHandler::create(Box::new(move |wv, args| {
                let (Some(wv), Some(args)) = (wv, args) else { return Ok(()) };
                let mut src = PWSTR::null();
                let mut text = PWSTR::null();
                args.Source(&mut src)?;
                if args.TryGetWebMessageAsString(&mut text).is_ok() {
                    let (src, text) = (take_pwstr(src), take_pwstr(text));
                    handle_message(&app, &wv, &src, &text);
                }
                Ok(())
            })),
            &mut token,
        );
    }
}

// ───────────── 브리지 (보이지 않는 확장 페이지) ─────────────

thread_local! {
    /// 확장 ID → 브리지 웹뷰 이름
    static BRIDGES: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

fn bridge_label(ext: &str) -> String {
    format!("extbridge-{ext}")
}

/// 켜져 있고 오른쪽 클릭 메뉴를 쓰는 확장마다 브리지를 하나 둔다. 메인 스레드에서 부른다.
pub fn sync_bridges(app: &AppHandle) {
    let want: Vec<String> = extensions::list(app).into_iter().filter(|i| i.enabled && i.context_menus && !i.id.is_empty()).map(|i| i.id).collect();
    let have: Vec<String> = BRIDGES.with(|b| b.borrow().keys().cloned().collect());
    for ext in have.iter().filter(|e| !want.contains(e)) {
        if let Some(wv) = app.get_webview(&bridge_label(ext)) {
            let _ = wv.close();
        }
        BRIDGES.with(|b| b.borrow_mut().remove(ext));
        if let Some(m) = MENUS.lock().unwrap().as_mut() {
            m.remove(ext);
        }
    }
    for ext in want.iter().filter(|e| !have.contains(e)) {
        if let Err(e) = create_bridge(app, ext) {
            log::warn(format!("확장 {ext}의 브리지를 만들지 못함: {e}"));
        }
    }
}

fn create_bridge(app: &AppHandle, ext: &str) -> Result<(), String> {
    let win = app.get_window(browser::MAIN_WINDOW).ok_or("브라우저 창이 없습니다.")?;
    let url = extensions::page_url(ext, "tbs-compat/bridge.html")?;
    let mut b = tauri::webview::WebviewBuilder::new(bridge_label(ext), WebviewUrl::External(url))
        .data_directory(paths::webview_dir())
        .browser_extensions_enabled(true)
        .focused(false);
    if let Some(env) = browser::environment() {
        b = b.with_environment(env);
    }
    let wv = win.add_child(b, LogicalPosition::new(0.0, 0.0), LogicalSize::new(1.0, 1.0)).map_err(|e| e.to_string())?;
    let _ = wv.hide();
    let app2 = app.clone();
    let _ = wv.with_webview(move |p| {
        if let Ok(core) = unsafe { p.controller().CoreWebView2() } {
            listen(&app2, &core);
        }
    });
    BRIDGES.with(|b| b.borrow_mut().insert(ext.to_string(), bridge_label(ext)));
    Ok(())
}

/// 누른 메뉴 항목을 그 확장의 브리지에 전한다
pub fn send_click(app: &AppHandle, ext: &str, info: Value, page_url: String, page_title: String) {
    let Some(wv) = app.get_webview(&bridge_label(ext)) else { return };
    let msg = json!({ "click": { "info": info, "pageUrl": page_url, "pageTitle": page_title } }).to_string();
    let _ = wv.with_webview(move |p| unsafe {
        if let Ok(core) = p.controller().CoreWebView2() {
            let _ = core.PostWebMessageAsJson(&HSTRING::from(msg));
        }
    });
}

// ───────────── 오른쪽 클릭 메뉴 ─────────────

struct Target {
    page: String,
    frame: String,
    link: Option<String>,
    src: Option<String>,
    selection: Option<String>,
    editable: bool,
    media: Option<&'static str>,
}

fn target(t: &ICoreWebView2ContextMenuTarget) -> Target {
    unsafe {
        let s = |f: &dyn Fn(&mut PWSTR) -> windows::core::Result<()>| {
            let mut p = PWSTR::null();
            let _ = f(&mut p);
            take_pwstr(p)
        };
        let b = |f: &dyn Fn(&mut windows::core::BOOL) -> windows::core::Result<()>| {
            let mut v = windows::core::BOOL(0);
            let _ = f(&mut v);
            v.as_bool()
        };
        let mut kind = COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND::default();
        let _ = t.Kind(&mut kind);

        Target {
            page: s(&|p| t.PageUri(p)),
            frame: s(&|p| t.FrameUri(p)),
            link: b(&|v| t.HasLinkUri(v)).then(|| s(&|p| t.LinkUri(p))).filter(|u| !u.is_empty()),
            // WebView2 157은 이미지에서도 HasSourceUri를 거짓으로 준다. 이미지·영상·소리일 때는 주소를 직접 읽는다
            src: (b(&|v| t.HasSourceUri(v)) || is_media(kind)).then(|| s(&|p| t.SourceUri(p))).filter(|u| !u.is_empty()),
            selection: b(&|v| t.HasSelection(v)).then(|| s(&|p| t.SelectionText(p))),
            editable: b(&|v| t.IsEditable(v)),
            media: match kind {
                COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND_IMAGE => Some("image"),
                COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND_VIDEO => Some("video"),
                COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND_AUDIO => Some("audio"),
                _ => None,
            },
        }
    }
}

fn is_media(kind: COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND) -> bool {
    matches!(kind, COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND_IMAGE | COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND_VIDEO | COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND_AUDIO)
}

/// 이 클릭의 크롬 메뉴 "contexts" (selection, link, image, editable, frame, page)
fn contexts(t: &Target) -> Vec<&'static str> {
    let mut c = vec!["all"];
    if t.selection.is_some() {
        c.push("selection");
    }
    if t.link.is_some() {
        c.push("link");
    }
    if t.editable {
        c.push("editable");
    }
    if let Some(m) = t.media {
        c.push(m);
    }
    if !t.frame.is_empty() && t.frame != t.page {
        c.push("frame");
    }
    if c.len() == 1 {
        c.push("page");
    }
    c
}

fn shows(item: &MenuItem, t: &Target, ctx: &[&str]) -> bool {
    if item.visible == Some(false) {
        return false;
    }
    let wanted = item.contexts.clone().unwrap_or_else(|| vec!["page".into()]);
    if !wanted.iter().any(|w| ctx.contains(&w.as_str())) {
        return false;
    }
    if let Some(p) = &item.document_url_patterns {
        if !p.iter().any(|p| chrome_matches(p, &t.page)) {
            return false;
        }
    }
    if let Some(p) = &item.target_url_patterns {
        let target = t.link.as_deref().or(t.src.as_deref()).unwrap_or("");
        if !target.is_empty() && !p.iter().any(|p| chrome_matches(p, target)) {
            return false;
        }
    }
    true
}

/// 페이지의 오른쪽 클릭 메뉴에 확장 항목을 더한다 (WebView2의 ContextMenuRequested 처리기 안에서 부른다)
pub fn extend_menu(app: &AppHandle, tab: crate::state::TabId, args: &ICoreWebView2ContextMenuRequestedEventArgs) {
    let menus = MENUS.lock().unwrap().clone().unwrap_or_default();

    if menus.values().all(|(_, items)| items.is_empty()) {
        return;
    }
    let Some(env) = browser::environment().and_then(|e| e.cast::<ICoreWebView2Environment9>().ok()) else { return };
    let (Ok(list), Ok(t)) = (unsafe { args.MenuItems() }, unsafe { args.ContextMenuTarget() }) else { return };
    let t = target(&t);
    let ctx = contexts(&t);
    #[cfg(debug_assertions)]
    log::info(format!("[dev] 오른쪽 클릭: ctx={ctx:?} page={} src={:?} link={:?} 확장 {}개", t.page, t.src, t.link, menus.len()));
    let title = browser::snapshot(app).tabs.into_iter().find(|x| x.id == tab).map(|x| x.title).unwrap_or_default();
    let mut blocks: Vec<(String, String, Vec<MenuItem>, Vec<MenuItem>)> = Vec::new();
    let mut order: Vec<&String> = menus.keys().collect();
    order.sort();
    for ext in order {
        let (name, items) = &menus[ext];
        let shown: Vec<MenuItem> = items.iter().filter(|i| shows(i, &t, &ctx)).cloned().collect();
        let top: Vec<MenuItem> = shown.iter().filter(|i| i.parent_id.as_ref().is_none_or(|p| !shown.iter().any(|x| &x.id == p))).cloned().collect();
        if !top.is_empty() {
            blocks.push((ext.clone(), name.clone(), top, shown));
        }
    }
    if blocks.is_empty() {
        return;
    }
    #[cfg(debug_assertions)]
    log::info(format!("[dev] 오른쪽 클릭 메뉴에 확장 항목 넣음: {}", blocks.iter().map(|b| format!("{} {}개", b.1, b.3.len())).collect::<Vec<_>>().join(", ")));
    let make = |label: &str, kind: COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND| unsafe { env.CreateContextMenuItem(&HSTRING::from(label), None, kind).ok() };
    let mut count = 0u32;
    unsafe {
        let _ = list.Count(&mut count);
    }
    let mut at = count;
    let insert = |coll: &ICoreWebView2ContextMenuItemCollection, item: ICoreWebView2ContextMenuItem, idx: &mut u32| unsafe {
        if coll.InsertValueAtIndex(*idx, &item).is_ok() {
            *idx += 1;
        }
    };
    if let Some(sep) = make("", COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_SEPARATOR) {
        insert(&list, sep, &mut at);
    }
    for (ext, name, top, shown) in blocks {
        // 한 확장의 항목이 여럿이면 크롬처럼 확장 이름 아래로 묶는다
        let (parent, mut idx) = if top.len() > 1 {
            let Some(sub) = make(&name, COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_SUBMENU) else { continue };
            let children = unsafe { sub.Children() }.ok();
            insert(&list, sub, &mut at);
            match children {
                Some(c) => (c, 0u32),
                None => continue,
            }
        } else {
            (list.clone(), at)
        };
        for item in &top {
            add_item(app, &env, &parent, &mut idx, &ext, item, &shown, &t, &title);
        }
        if top.len() <= 1 {
            at = idx;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn add_item(
    app: &AppHandle,
    env: &ICoreWebView2Environment9,
    coll: &ICoreWebView2ContextMenuItemCollection,
    idx: &mut u32,
    ext: &str,
    item: &MenuItem,
    all: &[MenuItem],
    t: &Target,
    page_title: &str,
) {
    let children: Vec<&MenuItem> = all.iter().filter(|x| x.parent_id.as_deref() == Some(item.id.as_str())).collect();
    let kind = match (item.kind.as_deref(), children.is_empty()) {
        (_, false) => COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_SUBMENU,
        (Some("separator"), _) => COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_SEPARATOR,
        (Some("checkbox"), _) => COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_CHECK_BOX,
        (Some("radio"), _) => COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_RADIO,
        _ => COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_COMMAND,
    };
    let selection: String = t.selection.clone().unwrap_or_default().chars().take(32).collect();
    let label = item.title.clone().unwrap_or_default().replace("%s", &selection);
    let Ok(mi) = (unsafe { env.CreateContextMenuItem(&HSTRING::from(label), None, kind) }) else { return };
    unsafe {
        let _ = mi.SetIsEnabled(item.enabled != Some(false));
        if let Some(c) = item.checked {
            let _ = mi.SetIsChecked(c);
        }
    }
    if !children.is_empty() {
        if let Ok(sub) = unsafe { mi.Children() } {
            let mut j = 0u32;
            for c in children {
                add_item(app, env, &sub, &mut j, ext, c, all, t, page_title);
            }
        }
    } else if kind != COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_SEPARATOR {
        // 크롬의 OnClickData 모양
        let mut info = json!({ "menuItemId": item.id, "pageUrl": t.page, "editable": t.editable });
        if let Some(p) = &item.parent_id {
            info["parentMenuItemId"] = json!(p);
        }
        if t.frame != t.page && !t.frame.is_empty() {
            info["frameUrl"] = json!(t.frame);
        }
        if let Some(l) = &t.link {
            info["linkUrl"] = json!(l);
        }
        if let Some(s) = &t.src {
            info["srcUrl"] = json!(s);
        }
        if let Some(s) = &t.selection {
            info["selectionText"] = json!(s);
        }
        if let Some(m) = t.media {
            info["mediaType"] = json!(m);
        }
        if let Some(c) = item.checked {
            info["wasChecked"] = json!(c);
            info["checked"] = json!(!c);
        }
        let (app2, ext2, page, title) = (app.clone(), ext.to_string(), t.page.clone(), page_title.to_string());
        let mut token = 0i64;
        unsafe {
            let _ = mi.add_CustomItemSelected(
                &CustomItemSelectedEventHandler::create(Box::new(move |_, _| {
                    let (a, e, i, p, ti) = (app2.clone(), ext2.clone(), info.clone(), page.clone(), title.clone());
                    browser::later(&app2, move |_| send_click(&a, &e, i, p, ti));
                    Ok(())
                })),
                &mut token,
            );
        }
    }
    unsafe {
        if coll.InsertValueAtIndex(*idx, &mi).is_ok() {
            *idx += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chrome_patterns() {
        assert!(chrome_matches("https://*.dcinside.com/*", "https://gall.dcinside.com/board/lists/?id=x"));
        assert!(chrome_matches("https://*.dcinside.com/*", "https://dcinside.com/"));
        assert!(!chrome_matches("https://*.dcinside.com/*", "https://evil.com/?dcinside.com"));
        assert!(chrome_matches("*://*/*", "http://a.b/c"));
        assert!(chrome_matches("<all_urls>", "https://x.y/"));
        assert!(!chrome_matches("<all_urls>", "chrome-extension://abc/popup.html"));
    }

    #[test]
    fn chrome_pattern_vectors() {
        let v: Value = serde_json::from_str(include_str!("../../tests/rules-vectors.json")).unwrap();
        for c in v["chromePatterns"].as_array().unwrap() {
            let (p, u, want) = (c["pattern"].as_str().unwrap(), c["url"].as_str().unwrap(), c["match"].as_bool().unwrap());
            assert_eq!(chrome_matches(p, u), want, "{c}");
        }
    }

    #[test]
    fn patches_manifest_and_html_once() {
        let d = std::env::temp_dir().join(format!("tbs-compat-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("pages")).unwrap();
        fs::write(d.join("manifest.json"), r#"{ "manifest_version": 3, "name": "A", "background": { "service_worker": "bg.js", "type": "module" } }"#).unwrap();
        fs::write(d.join("popup.html"), "<!doctype html><html><head><title>p</title></head><body></body></html>").unwrap();
        fs::write(d.join("pages/opt.html"), "<html><HEAD lang=ko><script src=a.js></script></HEAD></html>").unwrap();
        assert!(patch(&d).unwrap());
        let m: Value = serde_json::from_str(&fs::read_to_string(d.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(m["background"]["service_worker"], "tbs-compat/sw.js");
        assert_eq!(fs::read_to_string(d.join("tbs-compat/sw.js")).unwrap(), "import \"/tbs-compat/sw-shim.js\";\nimport \"/bg.js\";\n");
        assert!(fs::read_to_string(d.join("popup.html")).unwrap().starts_with("<!doctype html><html><head><script src=\"/tbs-compat/page.js\"></script><title>"));
        assert!(fs::read_to_string(d.join("pages/opt.html")).unwrap().contains("<HEAD lang=ko><script src=\"/tbs-compat/page.js\"></script><script src=a.js>"));
        assert!(!patch(&d).unwrap(), "두 번째는 바뀌는 것이 없다");
    }

    #[test]
    fn menu_contexts() {
        let t = Target { page: "https://a.com/".into(), frame: "https://a.com/".into(), link: Some("https://b.com/x".into()), src: None, selection: None, editable: false, media: None };
        let ctx = contexts(&t);
        assert_eq!(ctx, vec!["all", "link"]);
        let page_only = MenuItem { id: "p".into(), ..Default::default() };
        assert!(!shows(&page_only, &t, &ctx), "기본 contexts는 page라 링크 위에서는 안 보인다");
        let link = MenuItem { id: "l".into(), contexts: Some(vec!["link".into()]), target_url_patterns: Some(vec!["https://b.com/*".into()]), ..Default::default() };
        assert!(shows(&link, &t, &ctx));
        let hidden = MenuItem { id: "h".into(), contexts: Some(vec!["all".into()]), visible: Some(false), ..Default::default() };
        assert!(!shows(&hidden, &t, &ctx));
    }
}
