//! 사이트 스크립트를 탭에 적용하는 일 (spec 6.2, 6.3). 메인 스레드에서만 부른다.
//!
//! - 주입: 콘텐츠 웹뷰를 만들 때(첫 이동 전에) 켜진 스크립트를 AddScriptToExecuteOnDocumentCreated로 등록한다.
//!   켜고 끄거나 다시 불러오면 기존 등록을 지우고 다시 등록한다. 열린 탭은 새로고침 뒤에 반영된다.
//! - 리소스 바꿔치기: 필요한 패턴에만 필터를 건다.
//! - 페이지의 부탁: 페이지도 같은 메시지를 보낼 수 있다고 보고 모두 의심한다. 보낸 페이지 주소가
//!   그 권한을 가진 켜진 스크립트의 matches에 맞을 때만 처리한다.

use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Mutex};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, EventTarget, Manager};
use webview2_com::{Microsoft::Web::WebView2::Win32::*, *};
use std::result::Result;
use windows::core::{Interface, HSTRING, PWSTR};

use crate::{
    browser::UI_WEBVIEW,
    downloads, native, paths,
    scripts::{self, Keys, Permission, Script, ScriptInfo},
    settings,
    state::TabId,
};

/// 단축키 표: 콘텐츠 웹뷰의 키 처리기(메인 스레드)가 바로 찾을 수 있게 따로 둔다
struct KeyEntry {
    keys: Keys,
    script: String,
    shortcut: String,
    matches: Vec<String>,
}

static KEYS: Mutex<Vec<KeyEntry>> = Mutex::new(Vec::new());

/// 페이지에서 눌린 키가 켜진 스크립트의 단축키이면 (스크립트 id, 단축키 id)
pub fn shortcut_for(vk: u32, ctrl: bool, shift: bool, alt: bool, page: &str) -> Option<(String, String)> {
    let k = Keys { vk, ctrl, shift, alt };
    KEYS.lock()
        .unwrap()
        .iter()
        .find(|e| e.keys == k && e.matches.iter().any(|p| scripts::pattern_matches(p, page)))
        .map(|e| (e.script.clone(), e.shortcut.clone()))
}

/// 읽어 들인 스크립트 목록
#[derive(Default)]
pub struct ScriptStore(pub Mutex<Vec<Script>>);

thread_local! {
    /// 탭마다 등록한 스크립트 id (지울 때 쓴다)
    static REGISTERED: RefCell<HashMap<TabId, Vec<String>>> = RefCell::new(HashMap::new());
    /// 탭마다 건 바꿔치기 필터
    static FILTERS: RefCell<HashMap<TabId, Vec<String>>> = RefCell::new(HashMap::new());
}

pub fn dir() -> std::path::PathBuf {
    paths::data_root().join("scripts")
}

fn store(app: &AppHandle) -> tauri::State<'_, ScriptStore> {
    app.state::<ScriptStore>()
}

pub fn list(app: &AppHandle) -> Vec<ScriptInfo> {
    store(app).0.lock().unwrap().iter().map(|s| s.info()).collect()
}

/// 폴더를 다시 읽는다
pub fn reload(app: &AppHandle) {
    let loaded = scripts::load_all(&dir());
    *KEYS.lock().unwrap() = loaded
        .iter()
        .filter(|s| s.active())
        .flat_map(|s| s.keys.iter().map(|(k, id)| KeyEntry { keys: *k, script: s.manifest.id.clone(), shortcut: id.clone(), matches: s.manifest.matches.clone() }))
        .collect();
    *store(app).0.lock().unwrap() = loaded;
    let _ = app.emit_to(EventTarget::webview(UI_WEBVIEW), "scripts_changed", list(app));
    let _ = app.emit_to(EventTarget::webview(crate::popup::LABEL), "scripts_changed", list(app));
}

/// 넣은 예시를 적어 두는 파일: 지운 예시를 다시 넣지 않으려는 것이다
#[derive(serde::Serialize, serde::Deserialize, Default)]
#[serde(default)]
struct SeededFile {
    version: u32,
    examples: Vec<String>,
}

/// 예시 스크립트를 넣는다 (spec 6.5, 6.9). 전에 넣은 적 없는 예시만 넣고, 같은 이름의 폴더가 있으면 건드리지 않는다.
/// 11단계 전에 설치한 사람은 처음 두 예시를 이미 넣은 것으로 본다.
pub fn seed_examples() {
    let root = dir();
    let record = root.join(".examples.json");
    let mut seeded = match crate::storage::load::<SeededFile>(&record).ok().flatten() {
        Some(f) => f,
        None if root.exists() => SeededFile { version: 1, examples: vec!["chess-pieces".into(), "youtube-screenshot".into()] },
        None => SeededFile::default(),
    };
    let ids: Vec<&str> = {
        let mut v: Vec<&str> = crate::examples::FILES.iter().filter_map(|(p, _)| p.split('/').next()).collect();
        v.dedup();
        v
    };
    let mut changed = false;
    for id in ids {
        if seeded.examples.iter().any(|e| e == id) {
            continue;
        }
        seeded.examples.push(id.to_string());
        changed = true;
        if root.join(id).exists() {
            continue;
        }
        for (path, bytes) in crate::examples::FILES.iter().filter(|(p, _)| p.split('/').next() == Some(id)) {
            let p = root.join(path);
            if let Some(parent) = p.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(p, bytes);
        }
    }
    if changed {
        seeded.version = 1;
        let _ = std::fs::create_dir_all(&root);
        let _ = crate::storage::save(&record, &seeded);
    }
}

/// 탭에 지금 켜진 스크립트를 등록하고, 다 되면 `then`을 부른다 (첫 이동은 그 뒤에 한다)
pub fn apply(app: &AppHandle, id: TabId, core: &ICoreWebView2, then: Box<dyn FnOnce()>) {
    unregister(id, core);
    let (codes, patterns): (Vec<String>, Vec<String>) = {
        let s = store(app);
        let all = s.0.lock().unwrap();
        let active: Vec<&Script> = all.iter().filter(|s| s.active()).collect();
        (
            active.iter().filter_map(|s| s.code.clone()).collect(),
            active.iter().flat_map(|s| s.overrides.iter().map(|o| o.0.clone())).collect(),
        )
    };
    for p in &patterns {
        let _ = unsafe { core.AddWebResourceRequestedFilter(&HSTRING::from(p.as_str()), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL) };
    }
    FILTERS.with(|f| f.borrow_mut().insert(id, patterns));
    if codes.is_empty() {
        then();
        return;
    }
    // 마지막 등록이 끝나면 then을 부른다
    let left = Rc::new(RefCell::new((codes.len(), Some(then))));
    for code in codes {
        let left_cb = left.clone();
        let handler = AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(move |err, sid| {
            if err.is_ok() {
                REGISTERED.with(|r| r.borrow_mut().entry(id).or_default().push(sid));
            }
            let mut l = left_cb.borrow_mut();
            l.0 -= 1;
            if l.0 == 0 {
                if let Some(f) = l.1.take() {
                    drop(l);
                    f();
                }
            }
            Ok(())
        }));
        if unsafe { core.AddScriptToExecuteOnDocumentCreated(&HSTRING::from(code), &handler) }.is_err() {
            let mut l = left.borrow_mut();
            l.0 -= 1;
            if l.0 == 0 {
                if let Some(f) = l.1.take() {
                    drop(l);
                    f();
                }
            }
        }
    }
}

fn unregister(id: TabId, core: &ICoreWebView2) {
    for sid in REGISTERED.with(|r| r.borrow_mut().remove(&id)).unwrap_or_default() {
        let _ = unsafe { core.RemoveScriptToExecuteOnDocumentCreated(&HSTRING::from(sid)) };
    }
    for p in FILTERS.with(|f| f.borrow_mut().remove(&id)).unwrap_or_default() {
        let _ = unsafe { core.RemoveWebResourceRequestedFilter(&HSTRING::from(p), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL) };
    }
}

pub fn forget(id: TabId) {
    REGISTERED.with(|r| r.borrow_mut().remove(&id));
    FILTERS.with(|f| f.borrow_mut().remove(&id));
}

/// 탭 웹뷰에 바꿔치기 처리기를 건다. 어떤 파일로 바꿀지는 요청이 올 때 지금 목록에서 찾는다.
pub fn attach_overrides(app: &AppHandle, core: &ICoreWebView2, env: ICoreWebView2Environment) -> Option<i64> {
    let app = app.clone();
    let mut token = 0i64;
    let handler = WebResourceRequestedEventHandler::create(Box::new(move |wv, args| unsafe {
        let (Some(wv), Some(args)) = (wv, args) else { return Ok(()) };
        let mut uri = PWSTR::null();
        args.Request()?.Uri(&mut uri)?;
        let uri = take_pwstr(uri);
        let page = native::source(&wv);
        let found = {
            let s = store(&app);
            let all = s.0.lock().unwrap();
            all.iter()
                .filter(|s| s.active() && s.applies_to(&page))
                .flat_map(|s| s.overrides.iter())
                .find(|o| scripts::pattern_matches(&o.0, &uri))
                .map(|o| (o.1.clone(), o.2))
        };
        let Some((bytes, ctype)) = found else { return Ok(()) };
        let stream = windows::Win32::UI::Shell::SHCreateMemStream(Some(&bytes));
        let headers = format!("Content-Type: {ctype}\r\nCache-Control: no-store\r\nAccess-Control-Allow-Origin: *");
        let resp = env.CreateWebResourceResponse(stream.as_ref(), 200, &HSTRING::from("OK"), &HSTRING::from(headers))?;
        args.SetResponse(&resp)?;
        Ok(())
    }));
    unsafe { core.add_WebResourceRequested(&handler, &mut token) }.ok()?;
    Some(token)
}

pub fn detach_overrides(core: &ICoreWebView2, token: i64) {
    let _ = unsafe { core.remove_WebResourceRequested(token) };
}

/// 디스크 캐시를 비운다. 바꿔치기는 캐시에 있는 파일에는 걸리지 않기 때문이다 (spec 6.2). 프로필 전체가 비워진다.
pub fn clear_disk_cache(core: &ICoreWebView2) {
    unsafe {
        if let Ok(p) = core.cast::<ICoreWebView2_13>().and_then(|c| c.Profile()).and_then(|p| p.cast::<ICoreWebView2Profile2>()) {
            let h = ClearBrowsingDataCompletedHandler::create(Box::new(|_| Ok(())));
            let _ = p.ClearBrowsingData(COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE, &h);
        }
    }
}

// ───────────── 페이지의 부탁 (spec 6.3) ─────────────

/// 스크립트에 단축키나 "이 탭에서 실행"을 알린다. 사용자 동작으로 부르므로 PiP·전체 화면 같은 것도 된다.
/// 메인 스레드에서, WebView2 이벤트 처리기 밖에서 부른다.
pub fn dispatch(core: &ICoreWebView2, script: &str, kind: &str, name: &str) {
    let expr = format!(
        "(()=>{{const m=window.__tbs_site_script_events;const f=m&&m.get({});if(f)f({},{});return !!f}})()",
        serde_json::to_string(script).unwrap(),
        serde_json::to_string(kind).unwrap(),
        serde_json::to_string(name).unwrap()
    );
    let params = json!({ "expression": expr, "userGesture": true, "returnByValue": true }).to_string();
    unsafe {
        let h = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(|_, _| Ok(())));
        let _ = core.CallDevToolsProtocolMethod(&HSTRING::from("Runtime.evaluate"), &HSTRING::from(params), &h);
    }
}

/// 설정 값이 바뀌었음을 열린 탭에 알린다 (tbs.onSettingsChanged)
pub fn broadcast_settings(app: &AppHandle, script: &str) {
    let values = store(app).0.lock().unwrap().iter().find(|s| s.manifest.id == script).map(|s| Value::Object(s.values.clone()));
    let Some(values) = values else { return };
    let msg = json!({ "__tbs_site_script_event": { "script": script, "kind": "settings", "data": values } }).to_string();
    for (_, c) in crate::browser::cores() {
        let _ = unsafe { c.PostWebMessageAsJson(&HSTRING::from(msg.as_str())) };
    }
}

/// "이 탭에서 실행": 켜져 있고 이 탭 주소에 맞는 스크립트의 그 동작만
pub fn run_action(app: &AppHandle, tab: TabId, script: &str, action: &str) -> Result<(), String> {
    let url = crate::browser::snapshot(app).tabs.into_iter().find(|t| t.id == tab).map(|t| t.url).unwrap_or_default();
    let ok = store(app).0.lock().unwrap().iter().any(|s| s.manifest.id == script && s.active() && s.applies_to(&url) && s.manifest.actions.iter().any(|a| a.id == action));
    if !ok {
        return Err("이 탭에서는 그 동작을 실행할 수 없습니다. 스크립트가 켜져 있고 이 사이트에 맞는지 확인해 주세요.".into());
    }
    let core = crate::browser::cores().into_iter().find(|(id, _)| *id == tab).map(|(_, c)| c).ok_or("탭이 아직 열리지 않았습니다. 탭을 한 번 눌러 연 뒤 다시 해 주세요.")?;
    dispatch(&core, script, "action", action);
    Ok(())
}

/// 페이지에서 온 메시지가 사이트 스크립트의 부탁이면 처리하고 답한다. 아니면 무시한다.
pub fn handle_message(app: &AppHandle, tab: TabId, core: &ICoreWebView2, source: &str, text: &str) {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return };
    let Some(req) = v.get("__tbs_site_script") else { return };
    let script_id = req.get("script").and_then(Value::as_str).unwrap_or("").to_string();
    let rid = req.get("rid").cloned().unwrap_or(Value::Null);
    let kind = req.get("kind").and_then(Value::as_str).unwrap_or("");
    let data = req.get("data").cloned().unwrap_or(Value::Null);
    let reply = |result: Result<Value, String>| {
        let msg = match result {
            Ok(r) => json!({ "__tbs_site_script_reply": { "script": script_id, "rid": rid, "ok": true, "result": r } }),
            Err(e) => json!({ "__tbs_site_script_reply": { "script": script_id, "rid": rid, "ok": false, "error": e } }),
        };
        let _ = unsafe { core.PostWebMessageAsJson(&HSTRING::from(msg.to_string())) };
    };
    let perm = match kind {
        "screenshot" => Permission::Screenshot,
        "saveFile" => Permission::SaveFile,
        "copyText" => Permission::CopyText,
        "tabs" => Permission::Tabs,
        "storage" | "setSetting" => Permission::Storage,
        _ => return reply(Err(format!("\"{kind}\"은(는) 부탁할 수 없는 일입니다. screenshot, saveFile, copyText, tabs, storage, setSetting만 됩니다."))),
    };
    // 그 권한을 가진 켜진 스크립트이고, 보낸 페이지 주소가 그 스크립트의 matches에 맞아야 한다
    let allowed = {
        let s = store(app);
        let all = s.0.lock().unwrap();
        all.iter()
            .find(|s| s.manifest.id == script_id)
            .map(|s| (s.active() && s.manifest.permissions.contains(&perm) && s.applies_to(source), s.info().name, s.dir.clone()))
    };
    let Some((true, name, dir)) = allowed else {
        crate::log::warn(format!("사이트 스크립트 부탁을 거절함: script={script_id} kind={kind} page={source}"));
        return reply(Err("이 페이지에서는 그 일을 부탁할 수 없습니다. 스크립트의 permissions와 matches를 확인해 주세요.".into()));
    };
    match perm {
        Permission::Tabs => {
            let action = data.get("action").and_then(Value::as_str).unwrap_or("").to_string();
            const ACTIONS: [&str; 12] = ["back", "forward", "reload", "close", "newTab", "reopen", "next", "prev", "home", "fullscreen", "maximize", "minimize"];
            if !ACTIONS.contains(&action.as_str()) {
                return reply(Err(format!("탭에 \"{action}\"은(는) 할 수 없습니다. {}만 됩니다.", ACTIONS.join(", "))));
            }
            reply(Ok(Value::Null));
            // 이 처리기 안에서 탭을 닫거나 만들지 않는다
            crate::browser::later(app, move |a| match action.as_str() {
                "back" => crate::browser::go_back(tab),
                "forward" => crate::browser::go_forward(tab),
                "reload" => crate::browser::reload(a, tab),
                "close" => crate::browser::close(a, tab),
                "newTab" => crate::shortcuts::run(a, "new_tab"),
                "reopen" => crate::shortcuts::run(a, "reopen_tab"),
                "next" => crate::browser::activate_neighbor(a, true),
                "prev" => crate::browser::activate_neighbor(a, false),
                // 홈: 이 탭을 새 탭 화면으로
                "home" => crate::browser::to_blank(a, tab),
                "fullscreen" => crate::browser::toggle_fullscreen(a),
                _ => {
                    if let Some(w) = a.get_window(crate::browser::MAIN_WINDOW) {
                        if action == "minimize" {
                            let _ = w.minimize();
                        } else if w.is_maximized().unwrap_or(false) {
                            let _ = w.unmaximize();
                        } else {
                            let _ = w.maximize();
                        }
                    }
                }
            });
        }
        Permission::Storage if kind == "setSetting" => {
            let key = data.get("key").and_then(Value::as_str).unwrap_or("").to_string();
            let value = data.get("value").cloned().unwrap_or(Value::Null);
            let result = {
                let s = store(app);
                let all = s.0.lock().unwrap();
                match all.iter().find(|s| s.manifest.id == script_id) {
                    Some(sc) => scripts::set_value(sc, &key, &value),
                    None => Err("스크립트를 찾지 못했습니다.".into()),
                }
            };
            match result {
                Ok(values) => {
                    reply(Ok(Value::Object(values)));
                    let id = script_id.clone();
                    crate::browser::later(app, move |a| settings_changed(a, &id));
                }
                Err(e) => reply(Err(e)),
            }
        }
        Permission::Storage => {
            let op = data.get("op").and_then(Value::as_str).unwrap_or("");
            let key = data.get("key").and_then(Value::as_str);
            reply(scripts::storage_op(&dir, op, key, data.get("value")));
        }
        Permission::CopyText => {
            let text = data.get("text").and_then(Value::as_str).unwrap_or("");
            reply(copy_text(text).map(|_| Value::Null));
        }
        Permission::SaveFile => {
            let name = scripts::safe_file_name(data.get("name").and_then(Value::as_str).unwrap_or("파일"));
            let bytes = if let Some(b64) = data.get("base64").and_then(Value::as_str) {
                let b64 = b64.split_once(";base64,").map(|(_, b)| b).unwrap_or(b64);
                match native::base64_decode(b64) {
                    Some(b) => b,
                    None => return reply(Err("base64 데이터가 잘못됐습니다.".into())),
                }
            } else {
                data.get("text").and_then(Value::as_str).unwrap_or("").as_bytes().to_vec()
            };
            reply(save(app, source, &name, &bytes));
        }
        Permission::Screenshot => {
            // 현재 탭 화면을 PNG로 찍는다. 끝나면 답한다.
            let app2 = app.clone();
            let core2 = core.clone();
            let src = source.to_string();
            let file = scripts::safe_file_name(&format!("{name} {}.png", stamp()));
            let rid2 = rid.clone();
            let sid = script_id.clone();
            capture(core, Box::new(move |r| {
                let result = r.and_then(|bytes| save(&app2, &src, &file, &bytes));
                let msg = match result {
                    Ok(r) => json!({ "__tbs_site_script_reply": { "script": sid, "rid": rid2, "ok": true, "result": r } }),
                    Err(e) => json!({ "__tbs_site_script_reply": { "script": sid, "rid": rid2, "ok": false, "error": e } }),
                };
                let _ = unsafe { core2.PostWebMessageAsJson(&HSTRING::from(msg.to_string())) };
            }));
        }
    }
}

/// 켜기 확인 창(시안 M)에 보낼 것. 크롬 확장 설치 확인과 같은 모양에 kind만 다르다
pub fn prompt_info(s: &Script) -> Value {
    let m = &s.manifest;
    let mut sites: Vec<String> = Vec::new();
    for p in &m.matches {
        let h = p.split_once("://").map(|(_, r)| r).unwrap_or(p).split('/').next().unwrap_or("");
        let h = h.trim_start_matches("*.").trim_start_matches("www.").to_string();
        if !h.is_empty() && h != "*" && !sites.contains(&h) {
            sites.push(h);
        }
    }
    json!({
        "kind": "script",
        "id": m.id, "name": s.info().name, "version": m.version, "description": m.description, "author": m.author,
        "enabled": m.enabled, "pinned": m.pinned, "icon": s.icon, "popup": null, "options": null,
        "matches": m.matches, "allSites": s.all_sites(), "sites": sites,
        "permissions": m.permissions.iter().map(|p| p.sentence()).collect::<Vec<_>>(),
        "error": null, "updateFrom": null,
    })
}

/// 폴더를 휴지통으로 옮긴다
pub fn to_recycle_bin(dir: &std::path::Path) -> Result<(), String> {
    use windows::Win32::UI::Shell::*;
    // 스크립트 폴더 안의 폴더만
    if dir.parent() != Some(self::dir().as_path()) {
        return Err("스크립트 폴더 밖의 것은 지우지 않습니다.".into());
    }
    let from: Vec<u16> = dir.as_os_str().encode_wide_nul2();
    let mut op = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: windows::core::PCWSTR(from.as_ptr()),
        fFlags: (FOF_ALLOWUNDO.0 | FOF_NOCONFIRMATION.0 | FOF_SILENT.0 | FOF_NOERRORUI.0) as u16,
        ..Default::default()
    };
    let r = unsafe { SHFileOperationW(&mut op) };
    if r != 0 || op.fAnyOperationsAborted.as_bool() {
        return Err(format!("스크립트 폴더를 휴지통으로 옮기지 못했습니다(코드 {r}). 폴더를 연 프로그램을 닫고 다시 해 주세요."));
    }
    Ok(())
}

/// 경로를 두 번 0으로 끝나는 UTF-16으로 (SHFileOperation의 pFrom)
trait WideNul2 {
    fn encode_wide_nul2(&self) -> Vec<u16>;
}

impl WideNul2 for std::ffi::OsStr {
    fn encode_wide_nul2(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().chain([0, 0]).collect()
    }
}

/// 설정 값이 바뀐 뒤: 다시 읽고, 열린 탭의 등록을 새로 하고(새로 연 페이지용), 열린 페이지에 알린다
pub fn settings_changed(app: &AppHandle, script: &str) {
    reload(app);
    crate::browser::reapply_scripts(app, false);
    broadcast_settings(app, script);
}

/// 파일 이름에 붙일 지금 시각 (예: 2026-11-10 09-40-12)
fn stamp() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{:04}-{:02}-{:02} {:02}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

const MAX_SAVE: usize = 200 * 1024 * 1024;

/// 다운로드 폴더에 저장하고 다운로드 목록에도 남긴다
fn save(app: &AppHandle, page: &str, name: &str, bytes: &[u8]) -> Result<Value, String> {
    if bytes.len() > MAX_SAVE {
        return Err("200MB보다 큰 파일은 저장하지 않습니다.".into());
    }
    let dir = settings::get(app).download_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("다운로드 폴더를 만들지 못했습니다({e})."))?;
    let path = downloads::free_path(&dir, name);
    std::fs::write(&path, bytes).map_err(|e| format!("파일을 저장하지 못했습니다({e}). 다운로드 폴더 권한을 확인해 주세요."))?;
    downloads::record_saved(app, page, &path, bytes.len() as i64);
    Ok(json!({ "path": path.to_string_lossy() }))
}

type CaptureDone = Box<dyn FnOnce(Result<Vec<u8>, String>)>;

fn capture(core: &ICoreWebView2, done: CaptureDone) {
    unsafe {
        let Some(stream) = windows::Win32::UI::Shell::SHCreateMemStream(None) else {
            return done(Err("화면을 담을 곳을 만들지 못했습니다.".into()));
        };
        let s2 = stream.clone();
        let done = Rc::new(RefCell::new(Some(done)));
        let d2 = done.clone();
        let h = CapturePreviewCompletedHandler::create(Box::new(move |err| {
            let r = if err.is_ok() { native::read_stream(&s2).ok_or("찍은 화면을 읽지 못했습니다.".to_string()) } else { Err(format!("화면을 찍지 못했습니다({err:?}).")) };
            if let Some(f) = d2.borrow_mut().take() {
                f(r);
            }
            Ok(())
        }));
        if let Err(e) = core.CapturePreview(COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG, &stream, &h) {
            if let Some(f) = done.borrow_mut().take() {
                f(Err(format!("화면을 찍지 못했습니다({e}).")));
            }
        }
    }
}

/// 글자를 클립보드에 넣는다
fn copy_text(text: &str) -> Result<(), String> {
    use windows::Win32::{
        Foundation::{HANDLE, HGLOBAL},
        System::{
            DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
            Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
        },
    };
    const CF_UNICODETEXT: u32 = 13;
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        OpenClipboard(None).map_err(|e| format!("클립보드를 열지 못했습니다({e}). 다른 프로그램이 쓰고 있을 수 있습니다."))?;
        let result = (|| -> Result<(), String> {
            EmptyClipboard().map_err(|e| e.to_string())?;
            let mem: HGLOBAL = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2).map_err(|e| e.to_string())?;
            let ptr = GlobalLock(mem) as *mut u16;
            if ptr.is_null() {
                return Err("메모리를 잠그지 못했습니다.".into());
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
            let _ = GlobalUnlock(mem);
            SetClipboardData(CF_UNICODETEXT, Some(HANDLE(mem.0))).map_err(|e| e.to_string())?;
            Ok(())
        })();
        let _ = CloseClipboard();
        result.map_err(|e| format!("클립보드에 넣지 못했습니다({e})."))
    }
}
