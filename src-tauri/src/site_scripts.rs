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
    scripts::{self, Permission, Script, ScriptInfo},
    settings,
    state::TabId,
};

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
    *store(app).0.lock().unwrap() = loaded;
    let _ = app.emit_to(EventTarget::webview(UI_WEBVIEW), "scripts_changed", list(app));
    let _ = app.emit_to(EventTarget::webview(crate::popup::LABEL), "scripts_changed", list(app));
}

/// 처음 실행할 때 예시 스크립트 두 개를 넣는다 (spec 6.5). 폴더가 이미 있으면 건드리지 않는다.
pub fn seed_examples() {
    let root = dir();
    if root.exists() {
        return;
    }
    for (path, bytes) in crate::examples::FILES {
        let p = root.join(path);
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(p, bytes);
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

/// 페이지에서 온 메시지가 사이트 스크립트의 부탁이면 처리하고 답한다. 아니면 무시한다.
pub fn handle_message(app: &AppHandle, core: &ICoreWebView2, source: &str, text: &str) {
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
        _ => return reply(Err(format!("\"{kind}\"은(는) 부탁할 수 없는 일입니다. screenshot, saveFile, copyText만 됩니다."))),
    };
    // 그 권한을 가진 켜진 스크립트이고, 보낸 페이지 주소가 그 스크립트의 matches에 맞아야 한다
    let allowed = {
        let s = store(app);
        let all = s.0.lock().unwrap();
        all.iter().find(|s| s.manifest.id == script_id).map(|s| (s.active() && s.manifest.permissions.contains(&perm) && s.applies_to(source), s.info().name))
    };
    let Some((true, name)) = allowed else {
        crate::log::warn(format!("사이트 스크립트 부탁을 거절함: script={script_id} kind={kind} page={source}"));
        return reply(Err("이 페이지에서는 그 일을 부탁할 수 없습니다. 스크립트의 permissions와 matches를 확인해 주세요.".into()));
    };
    match perm {
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
