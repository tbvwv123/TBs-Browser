//! 광고 차단 (spec 6.7). 확장의 네트워크 차단(declarativeNetRequest)이 WebView2에서 동작하지 않으므로 브라우저가 직접 한다.
//!
//! - 엔진: Brave의 adblock-rust. 목록은 EasyList, EasyPrivacy, List-KR. `adblock\`에 받아 두고 하루에 한 번 새로 받는다.
//! - 네트워크 차단: 탭마다 모든 요청에 WebResourceRequested 필터를 걸고, 막을 요청에는 빈 응답(403)을 준다.
//!   페이지 문서 자체는 막지 않는다. 서비스 워커를 거치는 요청은 가로채지 못한다 (0단계 9번).
//! - 화면 숨기기: 페이지마다 작은 스크립트(COSMETIC_JS)를 넣어 숨길 선택자를 호스트에 묻고 스타일로 숨긴다.
//! - 사이트별 끄기: settings.json의 adblock.disabledSites. 막은 개수는 탭마다 세고, 새 페이지를 불러오면 0부터 센다.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use adblock::{lists::{FilterSet, ParseOptions}, request::Request, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Url};
use webview2_com::{Microsoft::Web::WebView2::Win32::*, *};
use windows::core::{Interface, HSTRING, PWSTR};

use crate::{log, native, paths, settings, state::TabId, storage};
use std::result::Result;

pub struct ListDef {
    pub id: &'static str,
    pub name: &'static str,
    /// 설정 화면의 보조 글자 ("광고")
    pub kind: &'static str,
    url: &'static str,
}

pub const LISTS: [ListDef; 3] = [
    ListDef { id: "easylist", name: "EasyList", kind: "광고", url: "https://easylist.to/easylist/easylist.txt" },
    ListDef { id: "easyprivacy", name: "EasyPrivacy", kind: "추적기", url: "https://easylist.to/easylist/easyprivacy.txt" },
    ListDef {
        id: "listkr",
        name: "List-KR",
        kind: "한국 사이트",
        url: "https://cdn.jsdelivr.net/npm/@list-kr/filterslists@latest/dist/filterslist-uBlockOrigin.txt",
    },
];

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// 페이지에 넣는 화면 숨기기 스크립트. 숨길 선택자를 호스트에 묻고, 새로 생기는 class·id도 물어 본다.
const COSMETIC_JS: &str = r#"(() => {
  if (window.top !== window || !window.chrome || !chrome.webview || window.__tbsAdblock) return;
  window.__tbsAdblock = true;
  const post = (m) => chrome.webview.postMessage(JSON.stringify({ __tbs_adblock: m }));
  let style = null, generic = true;
  const seenC = new Set(), seenI = new Set();
  const add = (sels) => {
    if (!sels || !sels.length) return;
    if (!style) { style = document.createElement("style"); (document.head || document.documentElement).appendChild(style); }
    for (let i = 0; i < sels.length; i += 500) style.appendChild(document.createTextNode(sels.slice(i, i + 500).join(",") + "{display:none!important}"));
  };
  chrome.webview.addEventListener("message", (e) => {
    const d = e.data && e.data.__tbs_adblock_css;
    if (!d) return;
    if (d.generichide) generic = false;
    add(d.hide);
  });
  let timer = 0;
  const collect = (root) => {
    if (!generic) return;
    const classes = [], ids = [];
    for (const el of (root.querySelectorAll ? root.querySelectorAll("[class],[id]") : [])) {
      if (el.id && !seenI.has(el.id)) { seenI.add(el.id); ids.push(el.id); }
      for (const c of el.classList || []) if (!seenC.has(c)) { seenC.add(c); classes.push(c); }
    }
    if (classes.length || ids.length) post({ kind: "classes", classes, ids });
  };
  post({ kind: "page" });
  const start = () => {
    collect(document);
    new MutationObserver(() => { clearTimeout(timer); timer = setTimeout(() => collect(document), 300); })
      .observe(document.documentElement, { childList: true, subtree: true, attributes: true, attributeFilter: ["class", "id"] });
  };
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start, { once: true }); else start();
})();"#;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ListMeta {
    /// 받은 시각 (유닉스 ms)
    updated: i64,
    rules: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
struct MetaFile {
    version: u32,
    lists: HashMap<String, ListMeta>,
}

struct Shared {
    engine: Option<Arc<Engine>>,
    enabled: bool,
    disabled: HashSet<String>,
    meta: MetaFile,
    /// 목록을 받거나 엔진을 만드는 중
    busy: bool,
    error: Option<String>,
}

static SHARED: Mutex<Option<Shared>> = Mutex::new(None);

fn with<T>(f: impl FnOnce(&mut Shared) -> T) -> T {
    let mut g = SHARED.lock().unwrap();
    let s = g.get_or_insert_with(|| Shared { engine: None, enabled: true, disabled: HashSet::new(), meta: MetaFile::default(), busy: false, error: None });
    f(s)
}

thread_local! {
    /// 탭마다 이 페이지에서 막은 요청 수
    static COUNTS: RefCell<HashMap<TabId, u32>> = RefCell::new(HashMap::new());
    /// "*" 필터를 건 탭 (WebView2 객체)
    static FILTERED: RefCell<HashMap<TabId, ICoreWebView2>> = RefCell::new(HashMap::new());
}

fn dir() -> PathBuf {
    paths::data_root().join("adblock")
}

fn meta_file() -> PathBuf {
    dir().join("meta.json")
}

fn host_of(url: &str) -> Option<String> {
    let u = Url::parse(url).ok()?;
    if u.scheme() != "http" && u.scheme() != "https" {
        return None;
    }
    u.host_str().map(|h| h.trim_start_matches("www.").to_string())
}

// ───────────── 목록과 엔진 ─────────────

/// Windows의 URLDownloadToFile로 받는다 (시스템 프록시와 인증서를 그대로 쓴다)
fn download(url: &str) -> Result<String, String> {
    fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    let tmp = dir().join(format!(".download-{}", crate::history::now_ms()));
    let r = unsafe { windows::Win32::System::Com::Urlmon::URLDownloadToFileW(None, &HSTRING::from(url), &HSTRING::from(tmp.as_path()), 0, None) };
    let text = r.map_err(|e| format!("목록을 받지 못했습니다({e}). 인터넷 연결을 확인해 주세요.")).and_then(|_| fs::read_to_string(&tmp).map_err(|e| e.to_string()));
    let _ = fs::remove_file(&tmp);
    let text = text?;
    if !text.contains('\n') || text.len() < 200 {
        return Err("받은 목록이 비어 있습니다. 잠시 뒤 다시 갱신해 주세요.".into());
    }
    Ok(text)
}

fn enabled_lists(app: &AppHandle) -> Vec<&'static ListDef> {
    let s = settings::get(app).adblock;
    LISTS.iter().filter(|l| s.lists.get(l.id).copied().unwrap_or(true)).collect()
}

/// 켜진 목록으로 엔진을 새로 만든다. 받아 둔 파일이 없거나 하루가 지났으면(`refresh`일 때는 늘) 새로 받는다.
fn rebuild(app: &AppHandle, refresh: bool) {
    with(|s| s.busy = true);
    let mut meta = storage::load::<MetaFile>(&meta_file()).ok().flatten().unwrap_or_default();
    let now = crate::history::now_ms();
    let mut set = FilterSet::new(false);
    let mut errors = Vec::new();
    for l in enabled_lists(app) {
        let path = dir().join(format!("{}.txt", l.id));
        let stale = meta.lists.get(l.id).is_none_or(|m| now - m.updated > DAY_MS);
        let mut text = fs::read_to_string(&path).ok();
        if refresh || stale || text.is_none() {
            match download(l.url) {
                Ok(t) => {
                    let _ = fs::write(&path, &t);
                    meta.lists.insert(l.id.to_string(), ListMeta { updated: now, rules: 0 });
                    text = Some(t);
                }
                Err(e) => errors.push(format!("{}: {e}", l.name)),
            }
        }
        if let Some(t) = text {
            let rules = t.lines().filter(|x| !x.trim().is_empty() && !x.starts_with('!') && !x.starts_with('[')).count();
            meta.lists.entry(l.id.to_string()).or_default().rules = rules;
            set.add_filter_list(t, ParseOptions::default());
        }
    }
    meta.version = 1;
    let _ = storage::save(&meta_file(), &meta);
    let engine = Engine::new_with_filter_set(set);
    with(|s| {
        s.engine = Some(Arc::new(engine));
        s.meta = meta;
        s.busy = false;
        s.error = (!errors.is_empty()).then(|| errors.join("\n"));
    });
    if !errors.is_empty() {
        log::warn(format!("광고 차단 목록 갱신 실패: {}", errors.join(" / ")));
    }
    log::info("광고 차단 엔진 준비됨");
}

/// 시작할 때: 설정을 읽고, 엔진을 만들고, 하루에 한 번 목록을 새로 받는다
pub fn start(app: &AppHandle) {
    apply_settings(app);
    let app = app.clone();
    std::thread::spawn(move || loop {
        rebuild(&app, false);
        std::thread::sleep(Duration::from_secs(60 * 60));
    });
}

/// "지금 갱신"
pub fn update_now(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        rebuild(&app, true);
        crate::browser::later(&app, |a| crate::settings::notify(a));
    });
}

/// 설정이 바뀌었다: 켜짐, 끈 사이트를 맞추고, 열린 탭의 필터를 걸거나 뗀다 (메인 스레드에서 부른다)
pub fn apply_settings(app: &AppHandle) {
    let s = settings::get(app).adblock;
    let changed_lists = with(|sh| {
        let before = sh.enabled;
        sh.enabled = s.enabled;
        sh.disabled = s.disabled_sites.iter().cloned().collect();
        before != s.enabled
    });
    let _ = changed_lists;
    for id in crate::browser::loaded_tabs() {
        if let Some(core) = crate::browser::core_of(id) {
            set_filter(id, &core, s.enabled);
        }
    }
}

/// 켜진 목록이 바뀌면 엔진을 다시 만든다
pub fn lists_changed(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || rebuild(&app, false));
}

// ───────────── 탭에 걸기 ─────────────

fn set_filter(id: TabId, core: &ICoreWebView2, on: bool) {
    let has = FILTERED.with(|f| f.borrow().contains_key(&id));
    if on == has {
        return;
    }
    unsafe {
        let Ok(c22) = core.cast::<ICoreWebView2_22>() else { return };
        let r = if on {
            c22.AddWebResourceRequestedFilterWithRequestSourceKinds(
                windows::core::w!("*"),
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
            )
        } else {
            c22.RemoveWebResourceRequestedFilterWithRequestSourceKinds(
                windows::core::w!("*"),
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
            )
        };
        if r.is_ok() {
            FILTERED.with(|f| {
                if on {
                    f.borrow_mut().insert(id, core.clone());
                } else {
                    f.borrow_mut().remove(&id);
                }
            });
        }
    }
}

fn request_type(ctx: COREWEBVIEW2_WEB_RESOURCE_CONTEXT) -> &'static str {
    match ctx {
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET => "stylesheet",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE => "image",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA => "media",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT => "font",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT => "script",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_XML_HTTP_REQUEST | COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FETCH => "xhr",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_WEBSOCKET => "websocket",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_PING => "ping",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_CSP_VIOLATION_REPORT => "csp_report",
        _ => "other",
    }
}

/// 새 콘텐츠 웹뷰에 건다: 요청 처리기와 화면 숨기기 스크립트. 돌려주는 값은 처리기 토큰
pub fn attach(app: &AppHandle, core: &ICoreWebView2, env: ICoreWebView2Environment, id: TabId) -> Option<i64> {
    let _ = app;
    unsafe {
        let _ = core.AddScriptToExecuteOnDocumentCreated(&HSTRING::from(COSMETIC_JS), None);
    }
    let mut token = 0i64;
    let handler = WebResourceRequestedEventHandler::create(Box::new(move |wv, args| unsafe {
        let (Some(wv), Some(args)) = (wv, args) else { return Ok(()) };
        let mut ctx = COREWEBVIEW2_WEB_RESOURCE_CONTEXT::default();
        args.ResourceContext(&mut ctx)?;
        // 페이지 문서는 막지 않는다 (틀에 담긴 문서도. 그 안의 요청은 막는다)
        if ctx == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT {
            return Ok(());
        }
        let (engine, page_site_off) = {
            let page = native::source(&wv);
            let off = host_of(&page).is_none_or(|h| with(|s| !s.enabled || s.disabled.contains(&h)));
            (with(|s| s.engine.clone()), (off, page))
        };
        let (off, page) = page_site_off;
        let Some(engine) = engine.filter(|_| !off) else { return Ok(()) };
        let mut uri = PWSTR::null();
        args.Request()?.Uri(&mut uri)?;
        let uri = native_take(uri);
        if !uri.starts_with("http") {
            return Ok(());
        }
        let Ok(req) = Request::new(&uri, &page, request_type(ctx), "GET") else { return Ok(()) };
        if engine.check_network_request(&req).should_block() {
            let resp = env.CreateWebResourceResponse(None, 403, &HSTRING::from("Blocked"), &HSTRING::from(""))?;
            args.SetResponse(&resp)?;
            COUNTS.with(|c| *c.borrow_mut().entry(id).or_default() += 1);
        }
        Ok(())
    }));
    unsafe { core.add_WebResourceRequested(&handler, &mut token) }.ok()?;
    if with(|s| s.enabled) {
        set_filter(id, core, true);
    }
    Some(token)
}

pub fn detach(core: &ICoreWebView2, id: TabId, token: i64) {
    unsafe {
        let _ = core.remove_WebResourceRequested(token);
    }
    FILTERED.with(|f| f.borrow_mut().remove(&id));
    COUNTS.with(|c| c.borrow_mut().remove(&id));
}

/// 새 페이지를 불러오기 시작했다: 막은 개수를 0부터 센다
pub fn page_started(id: TabId) {
    COUNTS.with(|c| c.borrow_mut().insert(id, 0));
}

fn native_take(p: PWSTR) -> String {
    let s = unsafe { p.to_string().unwrap_or_default() };
    if !p.is_null() {
        unsafe { windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as _)) };
    }
    s
}

// ───────────── 화면 숨기기 ─────────────

/// 페이지의 화면 숨기기 스크립트가 보낸 메시지면 답하고 참을 돌려준다
pub fn handle_message(core: &ICoreWebView2, source: &str, text: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return false };
    let Some(m) = v.get("__tbs_adblock") else { return false };
    let off = host_of(source).is_none_or(|h| with(|s| !s.enabled || s.disabled.contains(&h)));
    let Some(engine) = with(|s| s.engine.clone()).filter(|_| !off) else { return true };
    let res = engine.url_cosmetic_resources(source);
    let reply = match m.get("kind").and_then(|k| k.as_str()) {
        Some("page") => {
            let hide: Vec<String> = res.hide_selectors.into_iter().collect();
            json!({ "__tbs_adblock_css": { "hide": hide, "generichide": res.generichide } })
        }
        Some("classes") if !res.generichide => {
            let list = |k: &str| -> Vec<String> {
                m.get(k).and_then(|x| x.as_array()).into_iter().flatten().filter_map(|x| x.as_str()).take(5000).map(str::to_string).collect()
            };
            let sel = engine.hidden_class_id_selectors(list("classes"), list("ids"), &res.exceptions);
            if sel.is_empty() {
                return true;
            }
            json!({ "__tbs_adblock_css": { "hide": sel } })
        }
        _ => return true,
    };
    unsafe {
        let _ = core.PostWebMessageAsJson(&HSTRING::from(reply.to_string()));
    }
    true
}

// ───────────── UI에 보일 상태 ─────────────

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub enabled: bool,
    /// 지금 탭의 사이트 ("youtube.com"). 웹 페이지가 아니면 None
    pub site: Option<String>,
    pub site_enabled: bool,
    pub count: u32,
    /// 목록을 마지막으로 받은 시각 (가장 오래된 것, 유닉스 ms). 아직 없으면 0
    pub updated: i64,
    pub busy: bool,
    pub error: Option<String>,
}

pub fn status(app: &AppHandle, tab: Option<TabId>) -> Status {
    let snap = crate::browser::snapshot(app);
    let url = tab.and_then(|t| snap.tabs.iter().find(|x| x.id == t)).map(|t| t.url.clone()).unwrap_or_default();
    let site = host_of(&url);
    let count = tab.map(|t| COUNTS.with(|c| c.borrow().get(&t).copied().unwrap_or(0))).unwrap_or(0);
    let names: Vec<&str> = enabled_lists(app).iter().map(|l| l.id).collect();
    with(|s| Status {
        enabled: s.enabled,
        site_enabled: site.as_ref().is_some_and(|h| !s.disabled.contains(h)),
        site,
        count,
        updated: names.iter().filter_map(|n| s.meta.lists.get(*n).map(|m| m.updated)).min().unwrap_or(0),
        busy: s.busy,
        error: s.error.clone(),
    })
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub kind: &'static str,
    pub enabled: bool,
    pub rules: usize,
    pub updated: i64,
}

pub fn lists(app: &AppHandle) -> Vec<ListInfo> {
    let on = settings::get(app).adblock.lists;
    let meta = with(|s| s.meta.clone());
    LISTS
        .iter()
        .map(|l| {
            let m = meta.lists.get(l.id).cloned().unwrap_or_default();
            ListInfo { id: l.id, name: l.name, kind: l.kind, enabled: on.get(l.id).copied().unwrap_or(true), rules: m.rules, updated: m.updated }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_blocks_and_hides() {
        let mut set = FilterSet::new(false);
        set.add_filter_list("||ads.example.com^\n##.banner-ad\nnews.kr##.side-ad\n@@||ads.example.com/ok.js".into(), ParseOptions::default());
        let e = Engine::new_with_filter_set(set);
        let blocked = Request::new("https://ads.example.com/a.js", "https://news.kr/", "script", "GET").unwrap();
        assert!(e.check_network_request(&blocked).should_block());
        let ok = Request::new("https://ads.example.com/ok.js", "https://news.kr/", "script", "GET").unwrap();
        assert!(!e.check_network_request(&ok).should_block());
        let res = e.url_cosmetic_resources("https://news.kr/a");
        assert!(res.hide_selectors.contains(".side-ad"));
        let generic = e.hidden_class_id_selectors(["banner-ad"], Vec::<String>::new(), &res.exceptions);
        assert_eq!(generic, vec![".banner-ad".to_string()]);
    }

    #[test]
    fn site_names() {
        assert_eq!(host_of("https://www.youtube.com/watch?v=1").as_deref(), Some("youtube.com"));
        assert_eq!(host_of("tbs://settings"), None);
    }
}
