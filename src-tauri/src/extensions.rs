//! 크롬 확장 (spec 6.6). 설치한 확장은 `extensions\<폴더>\`에 두고, 시작할 때마다 프로필에 등록한다.
//! WebView2는 확장 아이콘과 팝업을 그리지 않으므로 확장 패널(시안 K)과 팝업 창은 우리가 그린다.
//! 네트워크 차단 규칙(declarativeNetRequest)은 WebView2에서 동작하지 않는다 (2026-11-11 시험).

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{mpsc, Mutex},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};
use webview2_com::{Microsoft::Web::WebView2::Win32::*, *};
use windows::core::{Interface, HSTRING, PWSTR};
// webview2_com의 Result와 겹치지 않게 표준 Result를 쓴다
use std::result::Result;

use crate::{browser::UI_WEBVIEW, log, paths, storage};

const VERSION: u32 = 1;
/// 설치 확인을 기다리는 동안 풀어 둘 폴더 이름의 앞부분
const INCOMING: &str = ".incoming-";

/// 설치한 확장 하나 (state\extensions.json)
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Record {
    /// `extensions\` 아래 폴더 이름
    pub folder: String,
    /// WebView2가 준 확장 ID (폴더 경로로 정해진다)
    pub id: String,
    pub enabled: bool,
    pub pinned: bool,
    /// 지난 버전이 남긴 팝업 창 크기. 지금은 쓰지 않는다 (팝업은 내용 크기로 연다). 파일을 그대로 읽기 위해 둔다
    #[serde(skip_serializing_if = "Option::is_none")]
    pub popup_size: Option<[f64; 2]>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct RecordFile {
    version: u32,
    items: Vec<Record>,
}

/// UI에 보내는 확장 정보 (확장 패널, 설치 확인)
#[derive(Clone, Debug, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub enabled: bool,
    pub pinned: bool,
    /// 아이콘 (data: 주소). 없으면 UI가 이름 첫 글자로 그린다
    pub icon: Option<String>,
    /// 팝업 페이지 (`action.default_popup`)
    pub popup: Option<String>,
    /// 설정 페이지 (`options_ui.page`, `options_page`)
    pub options: Option<String>,
    /// 적용 주소 패턴 (콘텐츠 스크립트의 matches + host_permissions)
    pub matches: Vec<String>,
    pub all_sites: bool,
    /// 사람이 읽는 사이트 이름 ("dcinside.com")
    pub sites: Vec<String>,
    /// 설치 확인에 보일 권한 문장 (spec 6.3 표)
    pub permissions: Vec<String>,
    /// 매니페스트 버전이 3이 아니거나, 등록에 실패했을 때
    pub error: Option<String>,
    /// 설치 확인에서: 같은 이름으로 이미 설치한 것을 새 버전으로 바꾸는지, 그때 지금 버전
    pub update_from: Option<String>,
    /// "tabs" 권한이 있는지 (호환층이 지금 탭 주소를 알려 줘도 되는지)
    pub tabs_permission: bool,
    /// 오른쪽 클릭 메뉴(contextMenus)를 쓰는지 (호환층의 브리지를 둔다)
    pub context_menus: bool,
}

#[derive(Default)]
pub struct ExtStore {
    records: Mutex<Vec<Record>>,
    /// 설치 확인을 기다리는 확장: (풀어 둔 폴더, 같은 이름으로 이미 설치된 확장 ID)
    pending: Mutex<Option<(PathBuf, Option<String>)>>,
}

pub fn dir() -> PathBuf {
    paths::data_root().join("extensions")
}

fn file() -> PathBuf {
    paths::state_dir().join("extensions.json")
}

fn save(app: &AppHandle) -> Result<(), String> {
    let items = app.state::<ExtStore>().records.lock().unwrap().clone();
    storage::save(&file(), &RecordFile { version: VERSION, items })
}

/// 확장 목록이 바뀌었다: UI에 알리고, 오른쪽 클릭 메뉴 브리지를 맞춘다
fn emit(app: &AppHandle) {
    crate::browser::later(app, crate::ext_compat::sync_bridges);
    crate::browser::emit_ui(app, "extensions_changed", list(app));
    crate::popup::emit(app, "extensions_changed", list(app));
}

// ───────────── 매니페스트 읽기 ─────────────

/// `__MSG_extName__` 같은 이름을 `_locales`에서 찾아 바꾼다. 한국어, 기본 언어, 영어 순
fn localize(dir: &Path, manifest: &Value, text: &str) -> String {
    let Some(key) = text.strip_prefix("__MSG_").and_then(|t| t.strip_suffix("__")) else { return text.to_string() };
    let default = manifest.get("default_locale").and_then(|v| v.as_str()).unwrap_or("en");
    for locale in ["ko", default, "en"] {
        let path = dir.join("_locales").join(locale).join("messages.json");
        let Ok(raw) = fs::read_to_string(&path) else { continue };
        let Ok(messages) = serde_json::from_str::<Value>(raw.trim_start_matches('\u{feff}')) else { continue };
        // 키는 대소문자를 가리지 않는다 (크롬 규칙)
        if let Some(obj) = messages.as_object() {
            if let Some((_, m)) = obj.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)) {
                if let Some(s) = m.get("message").and_then(|v| v.as_str()) {
                    return s.to_string();
                }
            }
        }
    }
    key.to_string()
}

/// 아이콘 표에서 32px에 가장 가까운(작지 않은) 것을 고른다
fn pick_icon(icons: &Value) -> Option<String> {
    if let Some(s) = icons.as_str() {
        return Some(s.to_string());
    }
    let obj = icons.as_object()?;
    let mut sizes: Vec<(u32, String)> = obj.iter().filter_map(|(k, v)| Some((k.parse().ok()?, v.as_str()?.to_string()))).collect();
    sizes.sort();
    sizes.iter().find(|(s, _)| *s >= 32).or(sizes.last()).map(|(_, p)| p.clone())
}

fn icon_data_url(dir: &Path, rel: &str) -> Option<String> {
    let path = crate::scripts::inside(dir, rel.trim_start_matches('/')).ok()?;
    let bytes = fs::read(&path).ok()?;
    let mime = match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("svg") => "image/svg+xml",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => "image/png",
    };
    Some(format!("data:{mime};base64,{}", crate::native::base64(&bytes)))
}

/// 모든 사이트를 뜻하는 패턴인지
fn is_all_sites(p: &str) -> bool {
    matches!(p, "<all_urls>" | "*://*/*" | "http://*/*" | "https://*/*" | "*://*/" | "https://*/" | "http://*/")
}

/// "https://*.dcinside.com/*" → "dcinside.com"
fn site_name(p: &str) -> Option<String> {
    let rest = p.split("://").nth(1)?;
    let host = rest.split('/').next()?.trim_start_matches("*.");
    (!host.is_empty() && host != "*").then(|| host.trim_start_matches("www.").to_string())
}

/// 크롬 권한 이름 → 설치 확인 문장 (spec 6.3 표)
fn permission_sentence(p: &str) -> Option<String> {
    Some(
        match p {
            "storage" | "unlimitedStorage" => "자기 설정을 저장함",
            "alarms" => "정해진 시각에 스스로 일을 할 수 있음",
            "contextMenus" => "오른쪽 클릭 메뉴에 항목을 더함",
            "declarativeNetRequest" | "declarativeNetRequestWithHostAccess" | "declarativeNetRequestFeedback" => {
                "네트워크 요청을 막거나 바꿈 (이 브라우저에서는 동작하지 않음)"
            }
            "tabs" => "열린 탭의 주소와 제목을 읽음",
            "notifications" => "알림을 띄움",
            // 사이트 권한과 화면 없는 권한은 따로 보이거나 보이지 않는다
            "scripting" | "activeTab" | "offscreen" => return None,
            other => return Some(other.to_string()),
        }
        .to_string(),
    )
}

/// 폴더의 `manifest.json`을 읽어 UI에 보일 정보를 만든다
pub fn read_manifest(dir: &Path) -> Result<Info, String> {
    let raw = fs::read_to_string(dir.join("manifest.json"))
        .map_err(|e| format!("manifest.json을 읽지 못했습니다({e}). 크롬 확장 폴더나 zip이 맞는지 확인해 주세요."))?;
    let m: Value = serde_json::from_str(raw.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("manifest.json 형식이 잘못되었습니다({e})."))?;
    let s = |key: &str| m.get(key).and_then(|v| v.as_str()).map(|t| localize(dir, &m, t)).unwrap_or_default();
    let name = s("name");
    if name.is_empty() {
        return Err("manifest.json에 이름(name)이 없습니다. 크롬 확장이 맞는지 확인해 주세요.".into());
    }
    let action = m.get("action").or_else(|| m.get("browser_action"));
    let icon = m
        .get("icons")
        .and_then(pick_icon)
        .or_else(|| action.and_then(|a| a.get("default_icon")).and_then(pick_icon))
        .and_then(|rel| icon_data_url(dir, &rel));
    let popup = action.and_then(|a| a.get("default_popup")).and_then(|v| v.as_str()).map(|p| p.trim_start_matches('/').to_string());
    let options = m
        .get("options_ui")
        .and_then(|o| o.get("page"))
        .or_else(|| m.get("options_page"))
        .and_then(|v| v.as_str())
        .map(|p| p.trim_start_matches('/').to_string());
    let mut matches: Vec<String> = Vec::new();
    for cs in m.get("content_scripts").and_then(|v| v.as_array()).into_iter().flatten() {
        for p in cs.get("matches").and_then(|v| v.as_array()).into_iter().flatten().filter_map(|v| v.as_str()) {
            if !matches.iter().any(|x| x == p) {
                matches.push(p.to_string());
            }
        }
    }
    for p in m.get("host_permissions").and_then(|v| v.as_array()).into_iter().flatten().filter_map(|v| v.as_str()) {
        if !matches.iter().any(|x| x == p) {
            matches.push(p.to_string());
        }
    }
    let all_sites = matches.iter().any(|p| is_all_sites(p));
    let mut sites: Vec<String> = Vec::new();
    if !all_sites {
        for n in matches.iter().filter_map(|p| site_name(p)) {
            if !sites.contains(&n) {
                sites.push(n);
            }
        }
    }
    let raw_perms: Vec<&str> = m.get("permissions").and_then(|v| v.as_array()).into_iter().flatten().filter_map(|v| v.as_str()).collect();
    let tabs_permission = raw_perms.contains(&"tabs");
    let context_menus = raw_perms.contains(&"contextMenus");
    let mut permissions: Vec<String> = Vec::new();
    if all_sites {
        permissions.push("모든 사이트의 내용을 읽고 바꿀 수 있음".into());
    }
    for p in m.get("permissions").and_then(|v| v.as_array()).into_iter().flatten().filter_map(|v| v.as_str()) {
        if let Some(sentence) = permission_sentence(p) {
            if !permissions.contains(&sentence) {
                permissions.push(sentence);
            }
        }
    }
    let mv = m.get("manifest_version").and_then(|v| v.as_u64()).unwrap_or(0);
    let author = match m.get("author") {
        Some(Value::String(a)) => a.clone(),
        Some(Value::Object(o)) => o.get("email").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
        _ => String::new(),
    };
    Ok(Info {
        name,
        version: s("version"),
        description: s("description"),
        author,
        icon,
        popup,
        options,
        matches,
        all_sites,
        sites,
        permissions,
        error: (mv != 3).then(|| format!("매니페스트 버전 {mv} 확장입니다. WebView2는 버전 3(MV3) 확장만 제대로 지원합니다.")),
        tabs_permission,
        context_menus,
        ..Info::default()
    })
}

/// 설치한 확장 목록 (설치한 순서)
pub fn list(app: &AppHandle) -> Vec<Info> {
    let records = app.state::<ExtStore>().records.lock().unwrap().clone();
    records
        .iter()
        .map(|r| {
            let mut info = read_manifest(&dir().join(&r.folder)).unwrap_or_else(|e| Info {
                name: r.folder.clone(),
                error: Some(e),
                ..Info::default()
            });
            info.id = r.id.clone();
            info.enabled = r.enabled;
            info.pinned = r.pinned;
            if r.id.is_empty() && info.error.is_none() {
                info.error = Some("확장을 등록하지 못했습니다. 프로그램을 다시 시작해 보고, 그래도 안 되면 확장을 지우고 다시 설치해 주세요.".into());
            }
            info
        })
        .collect()
}

// ───────────── WebView2 프로필 작업 ─────────────

/// UI 웹뷰의 프로필로 비동기 작업을 하고 결과를 기다린다. 메인 스레드 밖에서 부른다.
fn profile_op<T: Send + 'static>(
    app: &AppHandle,
    op: impl FnOnce(ICoreWebView2Profile7, mpsc::Sender<Result<T, String>>) + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = mpsc::channel();
    let ui = app.get_webview(UI_WEBVIEW).ok_or("브라우저 화면을 찾지 못했습니다. 프로그램을 다시 시작해 주세요.")?;
    let tx2 = tx.clone();
    ui.with_webview(move |p| unsafe {
        let profile = p
            .controller()
            .CoreWebView2()
            .and_then(|c| c.cast::<ICoreWebView2_13>())
            .and_then(|c| c.Profile())
            .and_then(|pr| pr.cast::<ICoreWebView2Profile7>());
        match profile {
            Ok(pr) => op(pr, tx2),
            Err(e) => {
                let _ = tx2.send(Err(format!("이 WebView2 런타임은 확장을 지원하지 않습니다({e}). Windows 업데이트로 WebView2를 새로 받아 주세요.")));
            }
        }
    })
    .map_err(|e| e.to_string())?;
    drop(tx);
    rx.recv_timeout(Duration::from_secs(30)).map_err(|_| "확장 작업이 끝나지 않았습니다. 잠시 뒤 다시 시도해 주세요.".to_string())?
}

fn pwstr(f: impl FnOnce(&mut PWSTR) -> windows::core::Result<()>) -> String {
    let mut p = PWSTR::null();
    let _ = f(&mut p);
    take_pwstr(p)
}

/// 프로필에 확장 폴더를 등록한다. 돌려주는 값은 확장 ID
fn add(app: &AppHandle, path: PathBuf) -> Result<String, String> {
    profile_op(app, move |profile, tx| unsafe {
        let tx2 = tx.clone();
        let h = ProfileAddBrowserExtensionCompletedHandler::create(Box::new(move |err, ext| {
            let _ = tx2.send(match ext {
                Some(e) if err.is_ok() => Ok(pwstr(|p| e.Id(p))),
                _ => Err(format!("확장을 등록하지 못했습니다({err:?}). 크롬 확장(MV3)이 맞는지 확인해 주세요.")),
            });
            Ok(())
        }));
        if let Err(e) = profile.AddBrowserExtension(&HSTRING::from(path.as_path()), &h) {
            let _ = tx.send(Err(format!("확장을 등록하지 못했습니다({e}).")));
        }
    })
}

/// 프로필에 등록된 확장 ID들
fn installed_ids(app: &AppHandle) -> Result<Vec<String>, String> {
    profile_op(app, |profile, tx| unsafe {
        let tx2 = tx.clone();
        let h = ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(move |err, list| {
            let mut ids = Vec::new();
            if let (true, Some(l)) = (err.is_ok(), list) {
                let mut n = 0u32;
                let _ = l.Count(&mut n);
                for i in 0..n {
                    if let Ok(e) = l.GetValueAtIndex(i) {
                        ids.push(pwstr(|p| e.Id(p)));
                    }
                }
            }
            let _ = tx2.send(Ok(ids));
            Ok(())
        }));
        if let Err(e) = profile.GetBrowserExtensions(&h) {
            let _ = tx.send(Err(format!("확장 목록을 읽지 못했습니다({e}).")));
        }
    })
}

/// 등록된 확장 하나에 일을 시킨다 (켜기·끄기·지우기)
fn with_extension(app: &AppHandle, id: String, act: impl FnOnce(ICoreWebView2BrowserExtension, mpsc::Sender<Result<(), String>>) + Send + 'static) -> Result<(), String> {
    profile_op(app, move |profile, tx| unsafe {
        let tx2 = tx.clone();
        let h = ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(move |_, list| {
            let found = list.and_then(|l| {
                let mut n = 0u32;
                let _ = l.Count(&mut n);
                (0..n).filter_map(|i| l.GetValueAtIndex(i).ok()).find(|e| pwstr(|p| e.Id(p)) == id)
            });
            match found {
                Some(e) => act(e, tx2),
                // 이미 없으면 할 일이 없다
                None => {
                    let _ = tx2.send(Ok(()));
                }
            }
            Ok(())
        }));
        if let Err(e) = profile.GetBrowserExtensions(&h) {
            let _ = tx.send(Err(format!("확장 목록을 읽지 못했습니다({e}).")));
        }
    })
}

fn enable(app: &AppHandle, id: &str, on: bool) -> Result<(), String> {
    with_extension(app, id.to_string(), move |e, tx| unsafe {
        let tx2 = tx.clone();
        let h = BrowserExtensionEnableCompletedHandler::create(Box::new(move |err| {
            let _ = tx2.send(err.map_err(|e| format!("확장을 {}지 못했습니다({e}).", if on { "켜" } else { "끄" })));
            Ok(())
        }));
        if let Err(err) = e.Enable(on, &h) {
            let _ = tx.send(Err(format!("확장을 바꾸지 못했습니다({err}).")));
        }
    })
}

// ───────────── 시작, 설치, 지우기 ─────────────

/// 시작할 때: 저장한 목록을 읽고, 프로필에 없는 확장은 다시 등록하고, 켜짐 상태를 맞춘다
pub fn start(app: &AppHandle) {
    let records = match storage::load::<RecordFile>(&file()) {
        Ok(Some(f)) => f.items,
        Ok(None) => Vec::new(),
        Err(msg) => {
            crate::library::notice(app, msg);
            Vec::new()
        }
    };
    *app.state::<ExtStore>().records.lock().unwrap() = records;
    cleanup_incoming();
    let app = app.clone();
    std::thread::spawn(move || {
        let installed = installed_ids(&app).unwrap_or_default();
        let records = app.state::<ExtStore>().records.lock().unwrap().clone();
        let mut changed = false;
        for r in &records {
            let path = dir().join(&r.folder);
            if !path.join("manifest.json").exists() {
                continue;
            }
            // 호환층을 끼운다. 이미 등록된 확장을 새로 고쳤으면 껐다 켜야 반영된다
            let patched = crate::ext_compat::patch(&path).unwrap_or_else(|e| {
                log::warn(format!("확장 {} 호환층을 끼우지 못함: {e}", r.folder));
                false
            });
            let id = if installed.contains(&r.id) {
                r.id.clone()
            } else {
                match add(&app, path) {
                    Ok(id) => id,
                    Err(e) => {
                        log::warn(format!("확장 {} 등록 실패: {e}", r.folder));
                        continue;
                    }
                }
            };
            if id != r.id {
                changed = true;
                if let Some(x) = app.state::<ExtStore>().records.lock().unwrap().iter_mut().find(|x| x.folder == r.folder) {
                    x.id = id.clone();
                }
            }
            if patched && installed.contains(&r.id) && r.enabled {
                let _ = enable(&app, &id, false);
            }
            if let Err(e) = enable(&app, &id, r.enabled) {
                log::warn(format!("확장 {} 켜짐 상태를 맞추지 못함: {e}", r.folder));
            }
        }
        if changed {
            let _ = save(&app);
        }
        emit(&app);
    });
}

fn cleanup_incoming() {
    for e in fs::read_dir(dir()).into_iter().flatten().flatten() {
        if e.file_name().to_string_lossy().starts_with(INCOMING) {
            let _ = fs::remove_dir_all(e.path());
        }
    }
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for e in fs::read_dir(from)? {
        let e = e?;
        let target = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &target)?;
        } else {
            fs::copy(e.path(), target)?;
        }
    }
    Ok(())
}

/// zip을 푼다. 폴더 밖으로 나가는 이름(../)은 건너뛴다
pub(crate) fn unzip(zip: &Path, to: &Path) -> Result<(), String> {
    let f = fs::File::open(zip).map_err(|e| format!("zip 파일을 열지 못했습니다({e})."))?;
    let mut archive = zip::ZipArchive::new(f).map_err(|e| format!("zip 파일이 아니거나 깨졌습니다({e})."))?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| format!("zip을 풀지 못했습니다({e})."))?;
        let Some(rel) = entry.enclosed_name() else { continue };
        let out = to.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut w = fs::File::create(&out).map_err(|e| format!("파일을 풀지 못했습니다({e})."))?;
            std::io::copy(&mut entry, &mut w).map_err(|e| format!("파일을 풀지 못했습니다({e})."))?;
        }
    }
    Ok(())
}

/// manifest.json이 바로 안에 없고 폴더 하나 안에 있으면 그 폴더 (zip에 폴더째 담은 경우)
fn manifest_root(dir: &Path) -> PathBuf {
    if dir.join("manifest.json").exists() {
        return dir.to_path_buf();
    }
    let subdirs: Vec<PathBuf> = fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    match subdirs.as_slice() {
        [only] if only.join("manifest.json").exists() => only.clone(),
        _ => dir.to_path_buf(),
    }
}

/// 고른 zip이나 폴더를 임시 폴더에 풀고 정보를 돌려준다. 확인을 받으면 `confirm`이 설치한다.
pub fn prepare(app: &AppHandle, source: &Path) -> Result<Info, String> {
    fs::create_dir_all(dir()).map_err(|e| format!("확장 폴더를 만들지 못했습니다({e}). 폴더 권한을 확인해 주세요."))?;
    let incoming = dir().join(format!("{INCOMING}{}", crate::history::now_ms()));
    let staged = if source.is_dir() {
        copy_dir(source, &incoming).map_err(|e| format!("확장 폴더를 복사하지 못했습니다({e})."))
    } else {
        unzip(source, &incoming)
    };
    if let Err(e) = staged {
        let _ = fs::remove_dir_all(&incoming);
        return Err(e);
    }
    let root = manifest_root(&incoming);
    let info = match read_manifest(&root) {
        Ok(i) => i,
        Err(e) => {
            let _ = fs::remove_dir_all(&incoming);
            return Err(e);
        }
    };
    // 같은 이름으로 이미 설치한 것이 있으면 업데이트로 본다
    let found = list(app).into_iter().find(|x| x.name == info.name);
    let mut info = info;
    info.update_from = found.as_ref().map(|x| x.version.clone());
    let existing = found.map(|x| x.id);
    if let Some(old) = app.state::<ExtStore>().pending.lock().unwrap().replace((incoming, existing)) {
        let _ = fs::remove_dir_all(old.0);
    }
    Ok(info)
}

/// 설치 확인을 기다리던 것을 꺼낸다 (답이 두 번 오면 두 번째는 할 일이 없다)
pub struct Pending(PathBuf, Option<String>);

pub fn take_pending(app: &AppHandle) -> Option<Pending> {
    app.state::<ExtStore>().pending.lock().unwrap().take().map(|(d, e)| Pending(d, e))
}

/// 설치 확인에 답했다. `accept`가 거짓이면 풀어 둔 것을 지운다.
pub fn confirm(app: &AppHandle, pending: Option<Pending>, accept: bool) -> Result<(), String> {
    let Some(Pending(incoming, existing)) = pending else { return Ok(()) };
    if !accept {
        let _ = fs::remove_dir_all(&incoming);
        return Ok(());
    }
    let root = manifest_root(&incoming);
    let info = read_manifest(&root)?;
    let result = install_into(app, &root, &info, existing);
    let _ = fs::remove_dir_all(&incoming);
    let r = result;
    emit(app);
    r
}

fn install_into(app: &AppHandle, root: &Path, info: &Info, existing: Option<String>) -> Result<(), String> {
    let store = app.state::<ExtStore>();
    // 업데이트: 같은 폴더(같은 ID)에 새 파일을 넣고 껐다 켠다. 확장의 저장 데이터는 남는다.
    if let Some(id) = existing.filter(|id| !id.is_empty()) {
        let folder = store.records.lock().unwrap().iter().find(|r| r.id == id).map(|r| r.folder.clone());
        if let Some(folder) = folder {
            let target = dir().join(&folder);
            let _ = enable(app, &id, false);
            let _ = fs::remove_dir_all(&target);
            copy_dir(root, &target).map_err(|e| format!("새 버전 파일을 넣지 못했습니다({e}). 프로그램을 다시 시작한 뒤 다시 설치해 주세요."))?;
            crate::ext_compat::patch(&target)?;
            let on = store.records.lock().unwrap().iter().find(|r| r.id == id).map(|r| r.enabled).unwrap_or(true);
            enable(app, &id, on)?;
            reload_tabs(app, &id);
            log::info(format!("확장 업데이트: {} {}", info.name, info.version));
            return Ok(());
        }
    }
    let base = crate::scripts::safe_file_name(&info.name).replace(' ', "-");
    let base = if base.is_empty() { "extension".to_string() } else { base };
    let mut folder = base.clone();
    let mut n = 2;
    while dir().join(&folder).exists() {
        folder = format!("{base}-{n}");
        n += 1;
    }
    let target = dir().join(&folder);
    copy_dir(root, &target).map_err(|e| format!("확장 파일을 넣지 못했습니다({e}). 디스크 공간과 폴더 권한을 확인해 주세요."))?;
    if let Err(e) = crate::ext_compat::patch(&target) {
        let _ = fs::remove_dir_all(&target);
        return Err(e);
    }
    // 같은 실행 중에 지운 확장을 같은 경로로 다시 등록하면 WebView2가 거절한다(접근 거부). 다른 폴더 이름으로 한 번 더 한다
    let (folder, target, added) = match add(app, target.clone()) {
        Ok(id) => (folder, target, Ok(id)),
        Err(_) => {
            let retry = format!("{folder}-{}", crate::history::now_ms() % 100000);
            let retry_path = dir().join(&retry);
            let r = fs::rename(&target, &retry_path).map_err(|e| e.to_string()).and_then(|_| add(app, retry_path.clone()));
            (retry, retry_path, r)
        }
    };
    let id = match added {
        Ok(id) => id,
        Err(e) => {
            let _ = fs::remove_dir_all(&target);
            return Err(e);
        }
    };
    store.records.lock().unwrap().push(Record { folder, id, enabled: true, pinned: false, popup_size: None });
    save(app)?;
    log::info(format!("확장 설치: {} {}", info.name, info.version));
    Ok(())
}

pub fn is_enabled(app: &AppHandle, id: &str) -> bool {
    app.state::<ExtStore>().records.lock().unwrap().iter().any(|r| r.id == id && r.enabled)
}

/// 확장 페이지를 연 탭을 다시 연다 (메인 스레드에서)
fn reload_tabs(app: &AppHandle, id: &str) {
    let id = id.to_string();
    crate::browser::later(app, move |a| crate::browser::reload_extension_tabs(a, &id));
}

pub fn set_enabled(app: &AppHandle, id: &str, on: bool) -> Result<(), String> {
    enable(app, id, on)?;
    if on {
        reload_tabs(app, id);
    } else {
        let (a, i) = (app.clone(), id.to_string());
        crate::browser::later(app, move |_| crate::popup::close_extension_of(&a, &i));
    }
    if let Some(r) = app.state::<ExtStore>().records.lock().unwrap().iter_mut().find(|r| r.id == id) {
        r.enabled = on;
    }
    save(app)?;
    emit(app);
    Ok(())
}

pub fn set_pinned(app: &AppHandle, id: &str, pinned: bool) -> Result<(), String> {
    if let Some(r) = app.state::<ExtStore>().records.lock().unwrap().iter_mut().find(|r| r.id == id) {
        r.pinned = pinned;
    }
    save(app)?;
    emit(app);
    Ok(())
}

/// 프로필에서 지우고 폴더도 지운다. 확장이 저장한 데이터도 WebView2가 지운다 (UI가 먼저 묻는다).
pub fn remove(app: &AppHandle, id: &str) -> Result<(), String> {
    let (a, i) = (app.clone(), id.to_string());
    crate::browser::later(app, move |_| crate::popup::close_extension_of(&a, &i));
    with_extension(app, id.to_string(), |e, tx| unsafe {
        let tx2 = tx.clone();
        let h = BrowserExtensionRemoveCompletedHandler::create(Box::new(move |err| {
            let _ = tx2.send(err.map_err(|e| format!("확장을 지우지 못했습니다({e}).")));
            Ok(())
        }));
        if let Err(err) = e.Remove(&h) {
            let _ = tx.send(Err(format!("확장을 지우지 못했습니다({err}).")));
        }
    })?;
    let folder = {
        let store = app.state::<ExtStore>();
        let mut records = store.records.lock().unwrap();
        let folder = records.iter().find(|r| r.id == id).map(|r| r.folder.clone());
        records.retain(|r| r.id != id);
        folder
    };
    if let Some(f) = folder {
        let _ = fs::remove_dir_all(dir().join(f));
    }
    save(app)?;
    emit(app);
    Ok(())
}

/// 확장 페이지 주소 (chrome-extension://<id>/<page>)
pub fn page_url(id: &str, page: &str) -> Result<tauri::Url, String> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_lowercase()) {
        return Err("확장 ID가 잘못되었습니다.".into());
    }
    tauri::Url::parse(&format!("chrome-extension://{id}/{}", page.trim_start_matches('/'))).map_err(|e| e.to_string())
}

/// 확장의 쪽(팝업, 설정) 이름을 찾는다
pub fn page_of(app: &AppHandle, id: &str, options: bool) -> Result<String, String> {
    let info = list(app).into_iter().find(|i| i.id == id).ok_or("확장을 찾지 못했습니다. 확장 패널을 다시 열어 주세요.")?;
    let page = if options { info.options } else { info.popup };
    page.ok_or_else(|| if options { "이 확장에는 설정 페이지가 없습니다.".into() } else { "이 확장에는 팝업이 없습니다.".into() })
}

/// 설치할 zip이나 폴더를 고르는 창. 메인 스레드에서 부른다.
pub fn pick(owner: Option<isize>, folder: bool) -> Option<PathBuf> {
    use windows::Win32::{
        Foundation::HWND,
        System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL},
        UI::Shell::{Common::COMDLG_FILTERSPEC, FileOpenDialog, IFileOpenDialog, FOS_PICKFOLDERS, SIGDN_FILESYSPATH},
    };
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_ALL).ok()?;
        if folder {
            let _ = dialog.SetOptions(dialog.GetOptions().ok()? | FOS_PICKFOLDERS);
            let _ = dialog.SetTitle(&HSTRING::from("크롬 확장 폴더 고르기 (manifest.json이 있는 폴더)"));
        } else {
            let (name, spec) = (HSTRING::from("크롬 확장 zip"), HSTRING::from("*.zip"));
            let filters = [COMDLG_FILTERSPEC { pszName: windows::core::PCWSTR(name.as_ptr()), pszSpec: windows::core::PCWSTR(spec.as_ptr()) }];
            let _ = dialog.SetFileTypes(&filters);
            let _ = dialog.SetTitle(&HSTRING::from("크롬 확장 zip 고르기"));
        }
        dialog.Show(owner.map(|h| HWND(h as _))).ok()?;
        let item = dialog.GetResult().ok()?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = name.to_string().ok();
        CoTaskMemFree(Some(name.0 as _));
        path.map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tbs-ext-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn reads_localized_manifest() {
        let d = temp("manifest");
        fs::write(
            d.join("manifest.json"),
            r#"{ "manifest_version": 3, "name": "__MSG_extName__", "version": "1.2.3", "default_locale": "en",
                 "action": { "default_popup": "popup.html", "default_icon": { "16": "i16.png", "48": "i48.png" } },
                 "options_ui": { "page": "/options.html" },
                 "permissions": ["storage", "alarms", "scripting", "contextMenus"],
                 "host_permissions": ["https://*.example.com/*"],
                 "content_scripts": [{ "matches": ["https://*.example.com/*", "https://other.kr/*"], "js": ["c.js"] }] }"#,
        )
        .unwrap();
        fs::create_dir_all(d.join("_locales/ko")).unwrap();
        fs::write(d.join("_locales/ko/messages.json"), r#"{ "extname": { "message": "예시 확장" } }"#).unwrap();
        fs::write(d.join("i48.png"), [137u8, 80, 78, 71]).unwrap();
        let i = read_manifest(&d).unwrap();
        assert_eq!(i.name, "예시 확장");
        assert_eq!(i.popup.as_deref(), Some("popup.html"));
        assert_eq!(i.options.as_deref(), Some("options.html"));
        assert!(i.icon.unwrap().starts_with("data:image/png;base64,"));
        assert_eq!(i.sites, vec!["example.com", "other.kr"]);
        assert!(!i.all_sites);
        assert_eq!(
            i.permissions,
            vec!["자기 설정을 저장함", "정해진 시각에 스스로 일을 할 수 있음", "오른쪽 클릭 메뉴에 항목을 더함"]
        );
        assert!(i.error.is_none());
    }

    #[test]
    fn all_sites_and_nested_zip_root() {
        let d = temp("root");
        let inner = d.join("my-ext");
        fs::create_dir_all(&inner).unwrap();
        fs::write(inner.join("manifest.json"), r#"{ "manifest_version": 2, "name": "A", "content_scripts": [{ "matches": ["<all_urls>"] }] }"#).unwrap();
        assert_eq!(manifest_root(&d), inner);
        let i = read_manifest(&inner).unwrap();
        assert!(i.all_sites);
        assert!(i.sites.is_empty());
        assert_eq!(i.permissions[0], "모든 사이트의 내용을 읽고 바꿀 수 있음");
        assert!(i.error.is_some(), "MV2는 알린다");
    }

    #[test]
    fn page_url_rejects_odd_ids() {
        assert!(page_url("abcdefghij", "popup.html").is_ok());
        assert!(page_url("../x", "popup.html").is_err());
        assert!(page_url("", "popup.html").is_err());
    }
}
