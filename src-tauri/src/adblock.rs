//! 광고 차단 (spec 6.7). 확장의 네트워크 차단(declarativeNetRequest)이 WebView2에서 동작하지 않으므로 브라우저가 직접 한다.
//!
//! - 엔진: Brave의 adblock-rust. 목록은 EasyList, EasyPrivacy, List-KR, uBlock filters. `adblock\`에 받아 두고 하루에 한 번 새로 받는다.
//! - 네트워크 차단: 탭마다 모든 요청에 WebResourceRequested 필터를 걸고, 막을 요청에는 빈 응답(403)을 준다.
//!   페이지 문서 자체는 막지 않는다. 서비스 워커가 보내는 요청도 이 필터에 걸린다 (WebView2 157에서 확인).
//! - 틀(iframe): 맨 위 문서가 아닌 문서 요청은 sub_frame으로 검사해 막는다.
//! - 화면 숨기기: 문서마다(틀 포함) 작은 스크립트(COSMETIC_JS)를 넣어 숨길 선택자를 호스트에 묻고 스타일로 숨긴다.
//! - 스크립틀릿(`+js(...)`): uBlock Origin의 스크립틀릿(정해 둔 판)을 숨은 웹뷰에서 읽어 엔진 자원으로 바꿔 두고
//!   (`scriptlets.json`), Brave 자원(resources.json)과 함께 엔진에 넣는다. 맨 위 문서가 이동을 시작할 때 그 주소에 맞는
//!   스크립트를 문서보다 먼저 돌도록 건다. 페이지 스크립트로 넣는 광고(유튜브 영상 광고 등)를 끈다.
//! - 사이트별 끄기: settings.json의 adblock.disabledSites. 막은 개수는 탭마다 세고, 새 페이지를 불러오면 0부터 센다.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use adblock::{lists::{FilterSet, ParseOptions}, request::Request, resources::{PermissionMask, Resource}, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, Url, WebviewUrl};
use webview2_com::{Microsoft::Web::WebView2::Win32::*, *};
use windows::core::{Interface, HSTRING, PWSTR};

use crate::{browser, log, native, paths, settings, state::TabId, storage};
use std::result::Result;

pub struct ListDef {
    pub id: &'static str,
    pub name: &'static str,
    /// 설정 화면의 보조 글자 ("광고")
    pub kind: &'static str,
    /// 받아서 이어 붙이는 주소들
    urls: &'static [&'static str],
    /// 믿는 목록: `trusted-` 스크립틀릿을 쓸 수 있다 (uBlock Origin이 자기 목록에만 허락하는 것과 같다)
    trusted: bool,
}

pub const LISTS: [ListDef; 4] = [
    ListDef { id: "easylist", name: "EasyList", kind: "광고", urls: &["https://easylist.to/easylist/easylist.txt"], trusted: false },
    ListDef { id: "easyprivacy", name: "EasyPrivacy", kind: "추적기", urls: &["https://easylist.to/easylist/easyprivacy.txt"], trusted: false },
    ListDef {
        id: "listkr",
        name: "List-KR",
        kind: "한국 사이트",
        urls: &["https://cdn.jsdelivr.net/npm/@list-kr/filterslists@latest/dist/filterslist-uBlockOrigin.txt"],
        trusted: false,
    },
    ListDef {
        id: "ubo",
        name: "uBlock filters",
        kind: "영상 광고·고치기",
        urls: &[
            "https://ublockorigin.github.io/uAssets/filters/filters.min.txt",
            "https://ublockorigin.github.io/uAssets/filters/quick-fixes.min.txt",
            "https://ublockorigin.github.io/uAssets/filters/unbreak.min.txt",
        ],
        trusted: true,
    },
];

/// 믿는 목록에 주는 권한 비트 (`requiresTrust` 스크립틀릿)
const TRUSTED: PermissionMask = PermissionMask::from_bits(1);
/// 스크립틀릿을 가져오는 uBlock Origin 판 (GPL-3.0. 받아서 이 컴퓨터에서만 쓴다)
const UBO_VERSION: &str = "1.75.4";
const SCRIPTLETS_LABEL: &str = "adblock-scriptlets";

const DAY_MS: i64 = 24 * 60 * 60 * 1000;
/// 스크립틀릿 자원 (Brave adblock-resources, MPL-2.0. 안의 스크립틀릿은 uBlock Origin 것이다)
const RESOURCES_URL: &str = "https://raw.githubusercontent.com/brave/adblock-resources/master/dist/resources.json";

/// 페이지에 넣는 화면 숨기기 스크립트. 숨길 선택자를 호스트에 묻고, 새로 생기는 class·id도 물어 본다.
const COSMETIC_JS: &str = r#"(() => {
  if (!window.chrome || !chrome.webview || window.__tbsAdblock) return;
  window.__tbsAdblock = true;
  const post = (m) => chrome.webview.postMessage(JSON.stringify({ __tbs_adblock: m }));
  let style = null, generic = true;
  const seenC = new Set(), seenI = new Set();
  // 브라우저가 못 읽는 선택자(uBO의 :has-text 같은 것)가 하나라도 끼면 같은 규칙 전체가 버려지므로 하나씩 걸러 낸다
  const ok = (s) => { try { return CSS.supports("selector(" + s + ")"); } catch (e) { return false; } };
  const add = (all) => {
    const sels = (all || []).filter(ok);
    if (!sels.length) return;
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

/// `scriptlets.json`: uBlock Origin 스크립틀릿을 엔진 자원 꼴로 바꾼 것
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct ScriptletsFile {
    version: String,
    made: i64,
    resources: Vec<Resource>,
}

/// 스크립틀릿을 만드는 중 (한 번에 하나만)
static MAKING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

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
    /// 탭마다 맨 위 문서가 지금 이동하는 주소 (틀 문서와 가르기 위해)
    static MAIN_NAV: RefCell<HashMap<TabId, String>> = RefCell::new(HashMap::new());
    /// 탭마다 걸어 둔 스크립틀릿의 WebView2 스크립트 ID
    static SCRIPTLETS: RefCell<HashMap<TabId, String>> = RefCell::new(HashMap::new());
    /// 엔진이 준비되면 할 일 (새 탭의 첫 이동)
    static WAITING: RefCell<Vec<Box<dyn FnOnce()>>> = RefCell::new(Vec::new());
}

/// 엔진이 준비된 뒤에 `f`를 부른다 (메인 스레드에서 부른다).
/// 시작할 때 다시 연 탭이 엔진보다 먼저 열리면 그 페이지에는 숨기기와 스크립틀릿이 걸리지 않는다.
/// 광고 차단이 꺼져 있거나 엔진이 있으면 바로, 5초가 지나면 더 기다리지 않는다.
pub fn when_ready(app: &AppHandle, f: Box<dyn FnOnce()>) {
    if with(|s| !s.enabled || s.engine.is_some()) {
        return f();
    }
    let first = WAITING.with(|w| {
        let mut w = w.borrow_mut();
        w.push(f);
        w.len() == 1
    });
    if first {
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(5));
            browser::later(&app, |_| flush_waiting());
        });
    }
}

fn flush_waiting() {
    for f in WAITING.with(|w| std::mem::take(&mut *w.borrow_mut())) {
        f();
    }
}

fn dir() -> PathBuf {
    paths::data_root().join("adblock")
}

fn meta_file() -> PathBuf {
    dir().join("meta.json")
}

/// 광고 차단의 사이트 이름 ("www."를 뺀 호스트). UI의 adblockSite와 함께 tests/rules-vectors.json(`adblockSites`)으로 시험한다
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
    // 목록은 여러 줄, 스크립틀릿 자원(JSON 배열)은 한 줄로 온다
    if text.len() < 200 || !(text.contains('\n') || text.trim_start().starts_with('[')) {
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
            let got: Result<Vec<String>, String> = l.urls.iter().map(|u| download(u)).collect();
            match got.map(|parts| parts.join("\n")) {
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
            let opts = ParseOptions { permissions: if l.trusted { TRUSTED } else { PermissionMask::default() }, ..Default::default() };
            set.add_filter_list(t, opts);
        }
    }
    // 스크립틀릿 자원
    let res_path = dir().join("resources.json");
    let res_stale = meta.lists.get("resources").is_none_or(|m| now - m.updated > DAY_MS);
    if refresh || res_stale || !res_path.exists() {
        match download(RESOURCES_URL) {
            Ok(t) => {
                let _ = fs::write(&res_path, &t);
                meta.lists.insert("resources".into(), ListMeta { updated: now, rules: 0 });
            }
            Err(e) => errors.push(format!("스크립틀릿 자원: {e}")),
        }
    }
    let mut resources: Vec<Resource> =
        fs::read_to_string(&res_path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    if let Some(m) = meta.lists.get_mut("resources") {
        m.rules = resources.len();
    }
    // uBlock Origin 스크립틀릿: 없거나 판이 다르면 숨은 웹뷰에서 새로 만든다 (다 되면 엔진을 다시 만든다)
    let sc: ScriptletsFile = storage::load(&scriptlets_file()).ok().flatten().unwrap_or_default();
    if sc.version != UBO_VERSION || sc.resources.is_empty() {
        request_scriptlets(app);
    }
    if !sc.resources.is_empty() {
        meta.lists.insert("scriptlets".into(), ListMeta { updated: sc.made, rules: sc.resources.len() });
        resources.extend(sc.resources);
    }
    meta.version = 1;
    let _ = storage::save(&meta_file(), &meta);
    let mut engine = Engine::new_with_filter_set(set);
    engine.use_resources(resources);
    with(|s| {
        s.engine = Some(Arc::new(engine));
        s.meta = meta;
        s.busy = false;
        s.error = (!errors.is_empty()).then(|| errors.join("\n"));
    });
    browser::later(app, |_| flush_waiting());
    if !errors.is_empty() {
        log::warn(format!("광고 차단 목록 갱신 실패: {}", errors.join(" / ")));
    }
    log::info("광고 차단 엔진 준비됨");
}

fn scriptlets_file() -> PathBuf {
    dir().join("scriptlets.json")
}

/// uBlock Origin의 스크립틀릿 모듈을 불러와 엔진 자원 꼴(JSON)로 바꾸는 식. 함수의 소스만 꺼내고 돌리지는 않는다.
fn scriptlets_expr() -> String {
    format!(
        r#"(async () => {{
  const m = await import("https://cdn.jsdelivr.net/gh/gorhill/uBlock@{UBO_VERSION}/src/js/resources/scriptlets.js");
  const b64 = (s) => {{ const u = new TextEncoder().encode(s); let t = ""; for (let i = 0; i < u.length; i += 0x8000) t += String.fromCharCode.apply(null, u.subarray(i, i + 0x8000)); return btoa(t); }};
  return JSON.stringify(m.builtinScriptlets.map((s) => {{
    const r = {{ name: s.name, aliases: s.aliases || [], kind: {{ mime: s.name.endsWith(".fn") ? "fn/javascript" : "application/javascript" }}, content: b64(s.fn.toString()), dependencies: (s.dependencies || []).filter((d) => typeof d === "string") }};
    if (s.requiresTrust) r.permission = 1;
    return r;
  }}));
}})()"#
    )
}

/// 숨은 웹뷰를 띄워 스크립틀릿을 만들게 한다 (어느 스레드에서 불러도 된다)
fn request_scriptlets(app: &AppHandle) {
    if MAKING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let a = app.clone();
    browser::later(app, move |app| {
        if let Err(e) = make_scriptlets(app) {
            scriptlets_done(app, Err(e));
        }
    });
    // 1분 안에 끝나지 않으면 그만둔다
    let a2 = a.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(60));
        if MAKING.load(std::sync::atomic::Ordering::SeqCst) {
            scriptlets_done(&a2, Err("시간이 너무 오래 걸립니다".into()));
        }
    });
}

fn make_scriptlets(app: &AppHandle) -> Result<(), String> {
    let win = app.get_window(browser::MAIN_WINDOW).ok_or("브라우저 창이 없습니다")?;
    let mut b = tauri::webview::WebviewBuilder::new(SCRIPTLETS_LABEL, WebviewUrl::External(Url::parse("about:blank").unwrap()))
        .data_directory(paths::webview_dir())
        .browser_extensions_enabled(true)
        .focused(false);
    if let Some(env) = browser::environment() {
        b = b.with_environment(env);
    }
    let wv = win.add_child(b, LogicalPosition::new(0.0, 0.0), LogicalSize::new(1.0, 1.0)).map_err(|e| e.to_string())?;
    let _ = wv.hide();
    // about:blank은 불러오기 끝 알림이 오지 않는다. 빈 문서가 자리 잡을 때까지 조금 기다렸다가 돌린다
    let app2 = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1500));
        let a = app2.clone();
        let _ = app2.run_on_main_thread(move || {
            let Some(wv) = a.get_webview(SCRIPTLETS_LABEL) else { return };
            let a2 = a.clone();
            let _ = wv.with_webview(move |p| unsafe {
                let Ok(core) = p.controller().CoreWebView2() else { return };
                let params = json!({ "expression": scriptlets_expr(), "awaitPromise": true, "returnByValue": true }).to_string();
                let h = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |err, res| {
                    scriptlets_done(&a2, if err.is_ok() { parse_scriptlets(&res) } else { Err(format!("{err:?}")) });
                    Ok(())
                }));
                let _ = core.CallDevToolsProtocolMethod(&HSTRING::from("Runtime.evaluate"), &HSTRING::from(params), &h);
            });
        });
    });
    Ok(())
}

/// DevTools의 Runtime.evaluate 결과에서 자원 목록을 꺼낸다
fn parse_scriptlets(res: &str) -> Result<Vec<Resource>, String> {
    let v: Value = serde_json::from_str(res).map_err(|e| e.to_string())?;
    if let Some(ex) = v.get("exceptionDetails") {
        let msg = ex.pointer("/exception/description").and_then(Value::as_str).unwrap_or("스크립트 오류");
        return Err(msg.lines().next().unwrap_or_default().to_string());
    }
    let text = v.pointer("/result/value").and_then(Value::as_str).ok_or("결과가 비어 있습니다")?;
    let list: Vec<Resource> = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if list.len() < 20 {
        return Err(format!("스크립틀릿이 {}개뿐입니다", list.len()));
    }
    Ok(list)
}

fn scriptlets_done(app: &AppHandle, result: Result<Vec<Resource>, String>) {
    if !MAKING.swap(false, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    browser::later(app, |app| {
        if let Some(wv) = app.get_webview(SCRIPTLETS_LABEL) {
            let _ = wv.close();
        }
    });
    match result {
        Ok(resources) => {
            let file = ScriptletsFile { version: UBO_VERSION.into(), made: crate::history::now_ms(), resources };
            if let Err(e) = storage::save(&scriptlets_file(), &file) {
                log::warn(format!("스크립틀릿을 저장하지 못함: {e}"));
                return;
            }
            log::info(format!("uBlock Origin {UBO_VERSION} 스크립틀릿 {}개를 만듦", file.resources.len()));
            let app = app.clone();
            std::thread::spawn(move || rebuild(&app, false));
        }
        Err(e) => {
            let msg = format!("스크립틀릿을 만들지 못했습니다({e}). 인터넷 연결을 확인하고 설정에서 '지금 갱신'을 눌러 주세요.");
            log::warn(&msg);
            with(|s| s.error = Some(msg));
        }
    }
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
    for (id, core) in browser::cores() {
        set_filter(id, &core, s.enabled);
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
        let mut uri = PWSTR::null();
        args.Request()?.Uri(&mut uri)?;
        let uri = take_pwstr(uri);
        // 맨 위 문서는 막지 않는다. 그 밖의 문서 요청은 틀(sub_frame)이다
        let mut kind = request_type(ctx);
        if ctx == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT {
            if MAIN_NAV.with(|m| m.borrow().get(&id).is_none_or(|u| *u == uri)) {
                return Ok(());
            }
            kind = "sub_frame";
        }
        let (engine, page_site_off) = {
            let page = native::source(&wv);
            let off = host_of(&page).is_none_or(|h| with(|s| !s.enabled || s.disabled.contains(&h)));
            (with(|s| s.engine.clone()), (off, page))
        };
        let (off, page) = page_site_off;
        let Some(engine) = engine.filter(|_| !off) else { return Ok(()) };
        if !uri.starts_with("http") {
            return Ok(());
        }
        let Ok(req) = Request::new(&uri, &page, kind, "GET") else { return Ok(()) };
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
    // 틀 안의 화면 숨기기 스크립트는 틀마다 메시지를 받는다 (맨 위 문서의 처리기로는 오지 않는다)
    if let Ok(c4) = core.cast::<ICoreWebView2_4>() {
        let mut t = 0i64;
        let top = core.clone();
        unsafe {
            let _ = c4.add_FrameCreated(
                &FrameCreatedEventHandler::create(Box::new(move |_, args| {
                    let top = top.clone();
                    let Some(frame) = args.and_then(|a| a.Frame().ok()).and_then(|f| f.cast::<ICoreWebView2Frame2>().ok()) else { return Ok(()) };
                    let mut t2 = 0i64;
                    let _ = frame.add_WebMessageReceived(
                        &FrameWebMessageReceivedEventHandler::create(Box::new(move |f, args| {
                            let (Some(f), Some(args)) = (f.and_then(|f| f.cast::<ICoreWebView2Frame2>().ok()), args) else { return Ok(()) };
                            let mut src = PWSTR::null();
                            let mut text = PWSTR::null();
                            args.Source(&mut src)?;
                            if args.TryGetWebMessageAsString(&mut text).is_ok() {
                                // 사이트별 끄기는 틀이 아니라 탭의 맨 위 페이지를 따른다
                                let page = native::source(&top);
                                if let Some(Some(reply)) = cosmetic_reply(&page, &take_pwstr(src), &take_pwstr(text)) {
                                    let _ = f.PostWebMessageAsJson(&HSTRING::from(reply));
                                }
                            }
                            Ok(())
                        })),
                        &mut t2,
                    );
                    Ok(())
                })),
                &mut t,
            );
        }
    }
    Some(token)
}

/// 맨 위 문서가 이동을 시작했다: 틀과 가를 주소를 남기고, 그 주소의 스크립틀릿을 문서보다 먼저 돌도록 건다
pub fn navigating(app: &AppHandle, id: TabId, core: &ICoreWebView2, uri: &str) {
    let _ = app;
    MAIN_NAV.with(|m| m.borrow_mut().insert(id, uri.to_string()));
    // 앞 페이지의 스크립틀릿은 뗀다
    if let Some(old) = SCRIPTLETS.with(|s| s.borrow_mut().remove(&id)) {
        unsafe {
            let _ = core.RemoveScriptToExecuteOnDocumentCreated(&HSTRING::from(old));
        }
    }
    let Some(host) = host_of(uri) else { return };
    let script = with(|s| (s.enabled && !s.disabled.contains(&host)).then(|| s.engine.clone()).flatten())
        .map(|e| e.url_cosmetic_resources(uri).injected_script)
        .unwrap_or_default();
    if script.trim().is_empty() {
        return;
    }
    // 다른 사이트로 넘어간 뒤의 문서에는 돌지 않게 호스트를 확인한다
    let full = Url::parse(uri).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
    let js = format!(
        "(()=>{{if(window.top!==window||location.hostname!=={}||window.__tbsScriptlets)return;window.__tbsScriptlets=true;const scriptletGlobals={{}};try{{{script}\n}}catch(e){{}}}})();",
        serde_json::to_string(&full).unwrap_or_default()
    );
    unsafe {
        let h = AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(move |err, sid| {
            // 그새 웹뷰를 닫았으면(detach) 남기지 않는다
            if err.is_ok() && MAIN_NAV.with(|m| m.borrow().contains_key(&id)) {
                SCRIPTLETS.with(|s| s.borrow_mut().insert(id, sid));
            }
            Ok(())
        }));
        let _ = core.AddScriptToExecuteOnDocumentCreated(&HSTRING::from(js), &h);
    }
}

pub fn detach(core: &ICoreWebView2, id: TabId, token: i64) {
    unsafe {
        let _ = core.remove_WebResourceRequested(token);
    }
    MAIN_NAV.with(|m| m.borrow_mut().remove(&id));
    SCRIPTLETS.with(|s| s.borrow_mut().remove(&id));
    FILTERED.with(|f| f.borrow_mut().remove(&id));
    COUNTS.with(|c| c.borrow_mut().remove(&id));
}

/// 새 페이지를 불러오기 시작했다: 막은 개수를 0부터 센다
pub fn page_started(id: TabId) {
    COUNTS.with(|c| c.borrow_mut().insert(id, 0));
}

/// 탭마다 남겨 둔 것의 (이름, 탭 id). 개발용 `orphans` 확인에 쓴다
#[cfg(debug_assertions)]
pub fn tab_entries() -> Vec<(&'static str, TabId)> {
    let mut v: Vec<(&'static str, TabId)> = Vec::new();
    COUNTS.with(|m| v.extend(m.borrow().keys().map(|id| ("adblock.COUNTS", *id))));
    FILTERED.with(|m| v.extend(m.borrow().keys().map(|id| ("adblock.FILTERED", *id))));
    MAIN_NAV.with(|m| v.extend(m.borrow().keys().map(|id| ("adblock.MAIN_NAV", *id))));
    SCRIPTLETS.with(|m| v.extend(m.borrow().keys().map(|id| ("adblock.SCRIPTLETS", *id))));
    v
}

// ───────────── 화면 숨기기 ─────────────

/// 페이지의 화면 숨기기 스크립트가 보낸 메시지면 답하고 참을 돌려준다
pub fn handle_message(core: &ICoreWebView2, source: &str, text: &str) -> bool {
    match cosmetic_reply(source, source, text) {
        None => false,
        Some(reply) => {
            if let Some(r) = reply {
                unsafe {
                    let _ = core.PostWebMessageAsJson(&HSTRING::from(r));
                }
            }
            true
        }
    }
}

/// 화면 숨기기 메시지의 답. 이 메시지가 아니면 None, 답할 것이 없으면 Some(None).
/// `page`는 탭의 맨 위 페이지(사이트별 끄기를 가른다), `source`는 메시지를 보낸 문서(맨 위 또는 틀)
fn cosmetic_reply(page: &str, source: &str, text: &str) -> Option<Option<String>> {
    let v = serde_json::from_str::<Value>(text).ok()?;
    let m = v.get("__tbs_adblock")?;
    let off = host_of(page).is_none_or(|h| with(|s| !s.enabled || s.disabled.contains(&h)));
    let Some(engine) = with(|s| s.engine.clone()).filter(|_| !off) else { return Some(None) };
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
                return Some(None);
            }
            json!({ "__tbs_adblock_css": { "hide": sel } })
        }
        _ => return Some(None),
    };
    Some(Some(reply.to_string()))
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

    #[test]
    fn site_name_vectors() {
        let v: Value = serde_json::from_str(include_str!("../../tests/rules-vectors.json")).unwrap();
        for c in v["adblockSites"].as_array().unwrap() {
            assert_eq!(host_of(c["url"].as_str().unwrap()).as_deref(), c["site"].as_str(), "{c}");
        }
    }

    /// uBlock Origin 꼴(함수) 스크립틀릿과 trusted 권한
    #[test]
    fn scriptlets_and_trust() {
        // 내용은 base64: "function setFlag(a){}", "function trustedFlag(a){}"
        let res = |name: &str, content: &str, permission: u8| {
            let v = json!({ "name": name, "kind": { "mime": "application/javascript" }, "content": content, "permission": permission });
            serde_json::from_value::<Resource>(v).unwrap()
        };
        let reply = parse_scriptlets(&json!({ "result": { "value": "[]" } }).to_string());
        assert!(reply.is_err());
        let mut set = FilterSet::new(false);
        set.add_filter_list("plain.kr##+js(set-flag, 1)\nplain.kr##+js(trusted-flag, 2)".into(), ParseOptions::default());
        let opts = ParseOptions { permissions: TRUSTED, ..Default::default() };
        set.add_filter_list("trusted.kr##+js(trusted-flag, 3)".into(), opts);
        let mut e = Engine::new_with_filter_set(set);
        e.use_resources([res("set-flag.js", "ZnVuY3Rpb24gc2V0RmxhZyhhKXt9", 0), res("trusted-flag.js", "ZnVuY3Rpb24gdHJ1c3RlZEZsYWcoYSl7fQ==", 1)]);
        let plain = e.url_cosmetic_resources("https://plain.kr/").injected_script;
        assert!(plain.contains("setFlag(\"1\")") && !plain.contains("trustedFlag"));
        let trusted = e.url_cosmetic_resources("https://trusted.kr/").injected_script;
        assert!(trusted.contains("trustedFlag(\"3\")"));
    }
}
