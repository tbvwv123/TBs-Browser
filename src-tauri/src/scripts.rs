//! 사이트 스크립트 (spec 6장). 확장 프로그램 대신 사이트별로 스크립트와 스타일을 끼워 넣는다.
//! 폴더: %LOCALAPPDATA%\TBs Browser\scripts\<id>\script.json

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// script.json (spec 6.1)
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub matches: Vec<String>,
    pub run_at: RunAt,
    pub js: Vec<String>,
    pub css: Vec<String>,
    pub resource_overrides: Vec<ResourceOverride>,
    pub permissions: Vec<Permission>,
    pub enabled: bool,
    pub pinned: bool,
    /// 아이콘 파일 (폴더 안의 PNG·SVG). 없으면 이름 첫 글자 (spec 6.4)
    pub icon: Option<String>,
    pub version: String,
    pub description: String,
    pub author: String,
    /// 설정 칸 (spec 6.1). 관리 화면이 설정 화면을 그린다
    pub settings: Vec<SettingField>,
    /// 단축키. 페이지에 포커스가 있어도 스크립트에 전달된다
    pub shortcuts: Vec<ShortcutDef>,
    /// 확장 패널의 "이 탭에서 실행" 버튼
    pub actions: Vec<ActionDef>,
}

/// 설정 칸 하나
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SettingField {
    pub key: String,
    #[serde(rename = "type")]
    pub kind: FieldKind,
    pub label: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub default: Value,
    /// 고르기 칸의 항목
    #[serde(default)]
    pub options: Vec<SelectOption>,
    /// 묶어 보일 작은 제목. 같은 제목이 이어지면 한 구역이다
    #[serde(default)]
    pub section: String,
    /// 숫자 칸 뒤에 붙는 단위 ("px")
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    /// 줄 앞에 보일 작은 아이콘 (폴더 안의 SVG·PNG). 읽은 뒤에는 데이터 URL
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub placeholder: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FieldKind {
    /// 켜기
    Toggle,
    /// 숫자
    Number,
    /// 글자
    Text,
    /// 글자 목록
    List,
    /// 고르기
    Select,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ShortcutDef {
    pub id: String,
    pub name: String,
    /// "Alt+P", "Ctrl+Shift+Y", "F9"
    pub keys: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ActionDef {
    pub id: String,
    pub name: String,
}

/// 단축키 하나를 가상 키와 Ctrl·Shift·Alt로
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keys {
    pub vk: u32,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// "Alt+P" 같은 글자를 읽는다. 글자·숫자 키에는 Ctrl이나 Alt가 있어야 한다(글 쓰는 것과 겹치지 않게).
/// 브라우저 단축키(spec 5.6)와 겹치면 안 된다.
pub fn parse_keys(text: &str) -> Result<Keys, String> {
    let bad = || format!("단축키 \"{text}\"를 읽지 못했습니다. \"Alt+P\", \"Ctrl+Shift+Y\", \"F9\"처럼 적어 주세요.");
    let mut k = Keys { vk: 0, ctrl: false, shift: false, alt: false };
    let parts: Vec<&str> = text.split('+').map(str::trim).collect();
    let (key, mods) = parts.split_last().ok_or_else(bad)?;
    for m in mods {
        match m.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => k.ctrl = true,
            "shift" => k.shift = true,
            "alt" => k.alt = true,
            _ => return Err(bad()),
        }
    }
    let up = key.to_ascii_uppercase();
    let fkey = up.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()).filter(|n| (1..=24).contains(n));
    if let Some(n) = fkey {
        k.vk = 0x6F + n;
    } else if up.len() == 1 && up.chars().all(|c| c.is_ascii_alphanumeric()) {
        k.vk = up.chars().next().unwrap() as u32;
        if !k.ctrl && !k.alt {
            return Err(format!("단축키 \"{text}\"에는 Ctrl이나 Alt가 있어야 합니다. 글자 키만으로는 글을 쓸 때 겹칩니다."));
        }
    } else {
        return Err(bad());
    }
    if crate::shortcuts::name(k.vk, k.ctrl, k.shift, k.alt).is_some() {
        return Err(format!("단축키 \"{text}\"는 브라우저 단축키와 겹칩니다. 다른 키를 골라 주세요."));
    }
    Ok(k)
}

impl Default for Manifest {
    fn default() -> Self {
        Manifest {
            id: String::new(),
            name: String::new(),
            matches: Vec::new(),
            run_at: RunAt::DocumentEnd,
            js: Vec::new(),
            css: Vec::new(),
            resource_overrides: Vec::new(),
            permissions: Vec::new(),
            enabled: true,
            pinned: false,
            icon: None,
            version: String::new(),
            description: String::new(),
            author: String::new(),
            settings: Vec::new(),
            shortcuts: Vec::new(),
            actions: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunAt {
    DocumentStart,
    DocumentEnd,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ResourceOverride {
    pub pattern: String,
    pub file: String,
}

/// 스크립트가 브라우저에 부탁할 수 있는 일 (spec 6.3). 이것이 전부다.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Permission {
    Screenshot,
    SaveFile,
    CopyText,
    /// 메시지가 온 그 탭에서만: 뒤로·앞으로, 새로고침, 닫기, 새 탭, 되살리기, 옆 탭, 홈. 창: 전체 화면, 최대화, 최소화
    Tabs,
    /// 스크립트별 저장 공간(storage.json)과 자기 설정 값 바꾸기
    Storage,
}

impl Permission {
    /// 켜기 확인 창에 보일 문장 (spec 6.3 표)
    pub fn sentence(self) -> &'static str {
        match self {
            Permission::Screenshot => "이 탭 화면을 찍어 다운로드 폴더에 저장함",
            Permission::SaveFile => "파일을 다운로드 폴더에 저장함",
            Permission::CopyText => "클립보드에 글자를 넣음",
            Permission::Tabs => "이 탭을 닫거나 뒤로 가게 하고, 새 탭을 열거나 창 크기를 바꿀 수 있음",
            Permission::Storage => "자기 설정을 저장함",
        }
    }
}

/// 읽어 들인 스크립트
#[derive(Clone, Debug)]
pub struct Script {
    pub manifest: Manifest,
    pub dir: PathBuf,
    /// 페이지에 넣을 코드 (js·css를 묶은 것). 오류가 있으면 None.
    pub code: Option<String>,
    /// 바꿔치기할 (주소 패턴, 파일 내용, 콘텐츠 형식)
    pub overrides: Vec<(String, Vec<u8>, &'static str)>,
    /// 읽다가 생긴 문제
    pub error: Option<String>,
    /// 아이콘 (데이터 URL)
    pub icon: Option<String>,
    /// 설정 칸 (아이콘은 데이터 URL로 바꾼 것)
    pub fields: Vec<SettingField>,
    /// 지금 설정 값 (기본값 위에 저장한 값)
    pub values: Map<String, Value>,
    /// 단축키 (읽은 것)
    pub keys: Vec<(Keys, String)>,
}

/// UI에 보내는 모양 (spec 6.4)
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptInfo {
    pub id: String,
    pub name: String,
    pub matches: Vec<String>,
    pub enabled: bool,
    pub pinned: bool,
    pub permissions: Vec<Permission>,
    pub error: Option<String>,
    pub icon: Option<String>,
    pub version: String,
    pub description: String,
    pub author: String,
    pub settings: Vec<SettingField>,
    pub values: Map<String, Value>,
    pub shortcuts: Vec<ShortcutDef>,
    pub actions: Vec<ActionDef>,
}

impl Script {
    pub fn info(&self) -> ScriptInfo {
        let m = &self.manifest;
        ScriptInfo {
            id: m.id.clone(),
            name: if m.name.is_empty() { m.id.clone() } else { m.name.clone() },
            matches: m.matches.clone(),
            enabled: m.enabled,
            pinned: m.pinned,
            permissions: m.permissions.clone(),
            error: self.error.clone(),
            icon: self.icon.clone(),
            version: m.version.clone(),
            description: m.description.clone(),
            author: m.author.clone(),
            settings: self.fields.clone(),
            values: self.values.clone(),
            shortcuts: m.shortcuts.clone(),
            actions: m.actions.clone(),
        }
    }

    /// 모든 사이트에서 도는지 (켤 때 묻는다, spec 6.1)
    pub fn all_sites(&self) -> bool {
        self.manifest.matches.iter().any(|m| m == "*" || m.starts_with("*://*") || m == "https://*" || m == "http://*" || m == "<all_urls>")
    }

    /// 켤 때 물어야 하는지: 모든 사이트이거나 새 권한(tabs, storage)이 있을 때 (spec 6.3)
    pub fn needs_confirm(&self) -> bool {
        self.all_sites() || self.manifest.permissions.iter().any(|p| matches!(p, Permission::Tabs | Permission::Storage))
    }

    /// 켜져 있고 오류가 없는지
    pub fn active(&self) -> bool {
        self.manifest.enabled && self.error.is_none()
    }

    pub fn applies_to(&self, url: &str) -> bool {
        self.manifest.matches.iter().any(|p| pattern_matches(p, url))
    }
}

/// `*`는 아무 글자나 (spec 6.1). 나머지는 그대로 비교한다.
pub fn pattern_matches(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !text.starts_with(first) || text.len() < first.len() + last.len() || !text.ends_with(last) {
        return false;
    }
    let mut rest = &text[first.len()..text.len() - last.len()];
    for mid in &parts[1..parts.len() - 1] {
        match rest.find(mid) {
            Some(i) => rest = &rest[i + mid.len()..],
            None => return false,
        }
    }
    true
}

/// 스크립트 폴더 안의 파일만 허용한다 (상위 폴더나 절대 경로는 안 된다)
pub fn inside(dir: &Path, file: &str) -> Result<PathBuf, String> {
    let rel = Path::new(file);
    if file.is_empty() || rel.is_absolute() || rel.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
        return Err(format!("파일 경로 \"{file}\"은(는) 쓸 수 없습니다. 스크립트 폴더 안의 파일 이름만 적어 주세요."));
    }
    Ok(dir.join(rel))
}

pub fn content_type(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "css" => "text/css",
        "js" => "text/javascript",
        "json" => "application/json",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        _ => "application/octet-stream",
    }
}

/// CSS 안의 `url("tbs-file:상대경로")`를 그 파일의 데이터 URL로 바꾼다.
/// 페이지는 내 컴퓨터의 파일을 읽을 수 없으므로, 폴더의 이미지를 스타일 덮어쓰기에 쓰려면 이렇게 넣는다.
fn inline_files(css: &str, dir: &Path) -> Result<String, String> {
    let css = strip_comments(css);
    let mut out = String::with_capacity(css.len());
    let mut rest = css.as_str();
    while let Some(i) = rest.find("tbs-file:") {
        out.push_str(&rest[..i]);
        let after = &rest[i + "tbs-file:".len()..];
        let end = after.find(|c: char| c == '"' || c == '\'' || c == ')' || c.is_whitespace()).unwrap_or(after.len());
        let file = &after[..end];
        let bytes = std::fs::read(inside(dir, file)?).map_err(|e| format!("\"{file}\"을(를) 읽지 못했습니다({e})."))?;
        out.push_str(&format!("data:{};base64,{}", content_type(file), crate::native::base64(&bytes)));
        rest = &after[end..];
    }
    out.push_str(rest);
    Ok(out)
}

/// CSS 주석(/* ... */)을 지운다. 주석 안의 글자를 파일 이름으로 읽지 않게 하려는 것이다.
fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(i) = rest.find("/*") {
        out.push_str(&rest[..i]);
        match rest[i + 2..].find("*/") {
            Some(j) => rest = &rest[i + 2 + j + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// 페이지에 넣을 코드. 주소가 `matches`에 맞을 때만 실행하도록 감싼다 (spec 6.2).
fn build_code(m: &Manifest, js: &str, css: &str, values: &Map<String, Value>) -> String {
    let id = serde_json::to_string(&m.id).unwrap();
    let patterns = serde_json::to_string(&m.matches).unwrap();
    let css = serde_json::to_string(css).unwrap();
    let values = serde_json::to_string(values).unwrap();
    let end = m.run_at == RunAt::DocumentEnd;
    format!(
        r#"(() => {{
const SCRIPT_ID = {id};
const PATTERNS = {patterns};
const match = (p, s) => new RegExp("^" + p.split("*").map(x => x.replace(/[.+?^${{}}()|[\]\\]/g, "\\$&")).join(".*") + "$").test(s);
if (!PATTERNS.some(p => match(p, location.href))) return;
const KEY = "__tbs_site_script_" + SCRIPT_ID;
if (window[KEY]) return;
Object.defineProperty(window, KEY, {{ value: true }});
// 브라우저에 부탁하기 (spec 6.3): tbs.request("screenshot" | "saveFile" | "copyText" | "tabs" | "storage" | "setSetting", 값)
const pending = new Map();
let seq = 0;
let SETTINGS = Object.freeze({values});
const on = {{ shortcut: new Map(), action: new Map(), settings: [] }};
const fire = (kind, name, data) => {{
  if (kind === "settings") {{
    SETTINGS = Object.freeze(data || {{}});
    for (const f of on.settings) {{ try {{ f(SETTINGS); }} catch (e) {{ console.error("[사이트 스크립트 " + SCRIPT_ID + "]", e); }} }}
    return;
  }}
  const f = on[kind] && on[kind].get(name);
  if (f) {{ try {{ f(); }} catch (e) {{ console.error("[사이트 스크립트 " + SCRIPT_ID + "]", e); }} }}
}};
// 단축키와 "이 탭에서 실행"은 브라우저가 사용자 동작으로 이 함수를 부른다 (맨 위 문서만)
if (!window.__tbs_site_script_events) Object.defineProperty(window, "__tbs_site_script_events", {{ value: new Map() }});
window.__tbs_site_script_events.set(SCRIPT_ID, fire);
if (window.chrome && window.chrome.webview) {{
  window.chrome.webview.addEventListener("message", e => {{
    const ev = e.data && e.data.__tbs_site_script_event;
    if (ev && ev.script === SCRIPT_ID) return fire(ev.kind, ev.name, ev.data);
    const r = e.data && e.data.__tbs_site_script_reply;
    if (!r || r.script !== SCRIPT_ID || !pending.has(r.rid)) return;
    const p = pending.get(r.rid); pending.delete(r.rid);
    r.ok ? p.resolve(r.result) : p.reject(new Error(r.error));
  }});
}}
const request = (kind, data) => new Promise((resolve, reject) => {{
  if (!(window.chrome && window.chrome.webview)) return reject(new Error("브라우저에 부탁할 수 없는 페이지입니다."));
  if (window.top !== window) return reject(new Error("틀(iframe) 안에서는 브라우저에 부탁할 수 없습니다."));
  const rid = ++seq;
  pending.set(rid, {{ resolve, reject }});
  window.chrome.webview.postMessage(JSON.stringify({{ __tbs_site_script: {{ script: SCRIPT_ID, rid, kind, data: data || {{}} }} }}));
}});
const tbs = Object.freeze({{
  request,
  /** 설정 값 (spec 6.1 settings). 권한 없이 읽는다 */
  get settings() {{ return SETTINGS; }},
  onSettingsChanged(f) {{ on.settings.push(f); }},
  /** 자기 설정 값을 바꾼다 (storage 권한) */
  setSetting(key, value) {{ return request("setSetting", {{ key, value }}); }},
  onShortcut(id, f) {{ on.shortcut.set(id, f); }},
  onAction(id, f) {{ on.action.set(id, f); }},
  /** 이 탭에서: "back" | "forward" | "reload" | "close" | "newTab" | "reopen" | "next" | "prev" (tabs 권한) */
  tabs(action) {{ return request("tabs", {{ action }}); }},
  /** 스크립트별 저장 공간 (storage 권한) */
  storage: Object.freeze({{
    get(key) {{ return request("storage", {{ op: "get", key }}); }},
    set(key, value) {{ return request("storage", {{ op: "set", key, value }}); }},
    remove(key) {{ return request("storage", {{ op: "remove", key }}); }},
  }}),
}});
const CSS = {css};
const run = () => {{
  if (CSS) {{
    const st = document.createElement("style");
    st.dataset.tbsSiteScript = SCRIPT_ID;
    st.textContent = CSS;
    (document.head || document.documentElement).appendChild(st);
  }}
  try {{
    (function (tbs) {{
{js}
    }})(tbs);
  }} catch (e) {{ console.error("[사이트 스크립트 " + SCRIPT_ID + "]", e); }}
}};
if ({end} && document.readyState === "loading") document.addEventListener("DOMContentLoaded", run, {{ once: true }});
else if (document.documentElement) run();
else document.addEventListener("readystatechange", run, {{ once: true }});
}})();"#
    )
}

/// 폴더 하나를 읽는다
pub fn load_one(dir: &Path) -> Script {
    let folder_id = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let fail = |m: Manifest, e: String| Script {
        manifest: m,
        dir: dir.to_path_buf(),
        code: None,
        overrides: Vec::new(),
        error: Some(e),
        icon: None,
        fields: Vec::new(),
        values: Map::new(),
        keys: Vec::new(),
    };
    let text = match std::fs::read_to_string(dir.join("script.json")) {
        Ok(t) => t,
        Err(e) => return fail(Manifest { id: folder_id, ..Default::default() }, format!("script.json을 읽지 못했습니다({e}).")),
    };
    let mut m: Manifest = match serde_json::from_str(&text) {
        Ok(m) => m,
        Err(e) => {
            return fail(Manifest { id: folder_id, enabled: false, ..Default::default() }, format!("script.json 형식이 잘못됐습니다({e}). spec 6.1의 형식을 확인해 주세요."))
        }
    };
    if m.id.is_empty() {
        m.id = folder_id;
    }
    let read_all = |files: &[String]| -> Result<String, String> {
        let mut out = String::new();
        for f in files {
            let p = inside(dir, f)?;
            let t = std::fs::read_to_string(&p).map_err(|e| format!("\"{f}\"을(를) 읽지 못했습니다({e})."))?;
            out.push_str(&t);
            out.push('\n');
        }
        Ok(out)
    };
    type Loaded = (String, Vec<(String, Vec<u8>, &'static str)>, Option<String>, Vec<SettingField>, Map<String, Value>, Vec<(Keys, String)>);
    let result = (|| -> Result<Loaded, String> {
        if m.matches.is_empty() {
            return Err("matches가 비어 있습니다. 적용할 주소 패턴을 적어 주세요.".into());
        }
        let js = read_all(&m.js)?;
        let css = inline_files(&read_all(&m.css)?, dir)?;
        let mut overrides = Vec::new();
        for o in &m.resource_overrides {
            let bytes = std::fs::read(inside(dir, &o.file)?).map_err(|e| format!("\"{}\"을(를) 읽지 못했습니다({e}).", o.file))?;
            overrides.push((o.pattern.clone(), bytes, content_type(&o.file)));
        }
        let icon = m.icon.as_deref().map(|f| data_url(dir, f)).transpose()?;
        let mut fields = m.settings.clone();
        let mut keys_seen = std::collections::HashSet::new();
        for f in fields.iter_mut() {
            if f.key.is_empty() || !keys_seen.insert(f.key.clone()) {
                return Err(format!("settings의 key \"{}\"가 비었거나 겹칩니다.", f.key));
            }
            if f.kind == FieldKind::Select && f.options.is_empty() {
                return Err(format!("settings \"{}\"는 고르기 칸인데 options가 없습니다.", f.key));
            }
            if let Some(i) = f.icon.take() {
                f.icon = Some(data_url(dir, &i)?);
            }
        }
        let values = read_values(dir, &fields);
        let mut keys = Vec::new();
        for sc in &m.shortcuts {
            keys.push((parse_keys(&sc.keys)?, sc.id.clone()));
        }
        Ok((build_code(&m, &js, &css, &values), overrides, icon, fields, values, keys))
    })();
    match result {
        Ok((code, overrides, icon, fields, values, keys)) => {
            Script { manifest: m, dir: dir.to_path_buf(), code: Some(code), overrides, error: None, icon, fields, values, keys }
        }
        Err(e) => fail(m, e),
    }
}

/// 폴더 안 파일을 데이터 URL로 (아이콘. 256KB까지)
fn data_url(dir: &Path, file: &str) -> Result<String, String> {
    let bytes = std::fs::read(inside(dir, file)?).map_err(|e| format!("\"{file}\"을(를) 읽지 못했습니다({e})."))?;
    if bytes.len() > 256 * 1024 {
        return Err(format!("아이콘 \"{file}\"이(가) 256KB보다 큽니다. 작은 PNG나 SVG로 바꿔 주세요."));
    }
    Ok(format!("data:{};base64,{}", content_type(file), crate::native::base64(&bytes)))
}

// ───────────── 설정 값 (spec 6.1 settings) ─────────────

/// 설정 값 파일: `scripts\<id>\settings.json` = { "version": 1, "values": { ... } }
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct ValuesFile {
    version: u32,
    values: Map<String, Value>,
}

fn values_path(dir: &Path) -> PathBuf {
    dir.join("settings.json")
}

/// 기본값 위에 저장한 값을 얹는다. 형식이 맞지 않는 값은 기본값으로 둔다
fn read_values(dir: &Path, fields: &[SettingField]) -> Map<String, Value> {
    let saved = crate::storage::load::<ValuesFile>(&values_path(dir)).ok().flatten().unwrap_or_default().values;
    fields
        .iter()
        .map(|f| {
            let v = saved.get(&f.key).and_then(|v| coerce(f, v).ok()).unwrap_or_else(|| default_value(f));
            (f.key.clone(), v)
        })
        .collect()
}

fn default_value(f: &SettingField) -> Value {
    coerce(f, &f.default).unwrap_or_else(|_| match f.kind {
        FieldKind::Toggle => Value::Bool(false),
        FieldKind::Number => Value::from(f.min.unwrap_or(0.0)),
        FieldKind::Text => Value::String(String::new()),
        FieldKind::List => Value::Array(Vec::new()),
        FieldKind::Select => Value::String(f.options.first().map(|o| o.value.clone()).unwrap_or_default()),
    })
}

/// 값이 칸 종류에 맞는지 보고 맞춘다
pub fn coerce(f: &SettingField, v: &Value) -> Result<Value, String> {
    let wrong = || format!("\"{}\"에 맞지 않는 값입니다.", f.label);
    match f.kind {
        FieldKind::Toggle => v.as_bool().map(Value::Bool).ok_or_else(wrong),
        FieldKind::Number => {
            let n = v.as_f64().filter(|n| n.is_finite()).ok_or_else(wrong)?;
            if f.min.is_some_and(|m| n < m) || f.max.is_some_and(|m| n > m) {
                return Err(format!("\"{}\"는 {}~{} 사이여야 합니다.", f.label, f.min.map(|m| m.to_string()).unwrap_or_default(), f.max.map(|m| m.to_string()).unwrap_or_default()));
            }
            Ok(Value::from(n))
        }
        FieldKind::Text => {
            let t = v.as_str().ok_or_else(wrong)?;
            if t.chars().count() > 2000 {
                return Err(format!("\"{}\"는 2000자까지 됩니다.", f.label));
            }
            Ok(Value::String(t.to_string()))
        }
        FieldKind::List => {
            let a = v.as_array().ok_or_else(wrong)?;
            if a.len() > 5000 {
                return Err(format!("\"{}\"에는 5000개까지 넣을 수 있습니다.", f.label));
            }
            let mut out = Vec::new();
            for x in a {
                let t = x.as_str().ok_or_else(wrong)?.trim();
                if t.is_empty() || t.chars().count() > 500 {
                    return Err(wrong());
                }
                if !out.iter().any(|o: &Value| o.as_str() == Some(t)) {
                    out.push(Value::String(t.to_string()));
                }
            }
            Ok(Value::Array(out))
        }
        FieldKind::Select => {
            let t = v.as_str().ok_or_else(wrong)?;
            if !f.options.iter().any(|o| o.value == t) {
                return Err(wrong());
            }
            Ok(Value::String(t.to_string()))
        }
    }
}

/// 설정 값 하나를 바꿔 저장하고, 바뀐 전체 값을 돌려준다
pub fn set_value(script: &Script, key: &str, value: &Value) -> Result<Map<String, Value>, String> {
    let f = script.fields.iter().find(|f| f.key == key).ok_or_else(|| format!("\"{key}\"라는 설정 칸이 없습니다."))?;
    let v = coerce(f, value)?;
    let mut values = script.values.clone();
    values.insert(key.to_string(), v);
    crate::storage::save(&values_path(&script.dir), &ValuesFile { version: 1, values: values.clone() })?;
    Ok(values)
}

// ───────────── 저장 공간 (spec 6.3 storage) ─────────────

const STORAGE_MAX: usize = 5 * 1024 * 1024;

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct StorageFile {
    version: u32,
    data: Map<String, Value>,
}

/// `storage.json`에 읽고 쓴다. op: get(key가 없으면 전부) | set | remove
pub fn storage_op(dir: &Path, op: &str, key: Option<&str>, value: Option<&Value>) -> Result<Value, String> {
    let path = dir.join("storage.json");
    let mut file = crate::storage::load::<StorageFile>(&path).ok().flatten().unwrap_or_default();
    match op {
        "get" => Ok(match key {
            Some(k) => file.data.get(k).cloned().unwrap_or(Value::Null),
            None => Value::Object(file.data),
        }),
        "set" | "remove" => {
            let k = key.filter(|k| !k.is_empty()).ok_or("저장할 이름(key)이 없습니다.")?;
            if op == "set" {
                file.data.insert(k.to_string(), value.cloned().unwrap_or(Value::Null));
            } else {
                file.data.remove(k);
            }
            file.version = 1;
            let size = serde_json::to_vec(&file).map(|v| v.len()).unwrap_or(usize::MAX);
            if size > STORAGE_MAX {
                return Err("저장 공간(5MB)이 가득 찼습니다. 필요 없는 값을 지워 주세요.".into());
            }
            crate::storage::save(&path, &file)?;
            Ok(Value::Null)
        }
        _ => Err(format!("저장 공간에 \"{op}\"는 할 수 없습니다. get, set, remove만 됩니다.")),
    }
}

/// 스크립트 폴더 전체를 읽는다. 이름 순.
pub fn load_all(root: &Path) -> Vec<Script> {
    let mut out: Vec<Script> = std::fs::read_dir(root)
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir() && p.join("script.json").exists()).map(|p| load_one(&p)).collect())
        .unwrap_or_default();
    // id가 겹치면 뒤의 것을 오류로 표시한다
    let mut seen: HashMap<String, usize> = HashMap::new();
    for s in out.iter_mut() {
        let n = seen.entry(s.manifest.id.clone()).or_insert(0);
        *n += 1;
        if *n > 1 {
            s.error = Some(format!("id \"{}\"가 다른 스크립트와 겹칩니다. 폴더 이름과 id를 바꿔 주세요.", s.manifest.id));
            s.code = None;
        }
    }
    out.sort_by(|a, b| a.info().name.cmp(&b.info().name));
    out
}

/// script.json의 한 항목만 바꿔 저장한다 (다른 항목은 그대로 둔다)
pub fn set_flag(dir: &Path, key: &str, value: bool) -> Result<(), String> {
    let path = dir.join("script.json");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("script.json을 읽지 못했습니다({e})."))?;
    let mut v: Value = serde_json::from_str(&text).map_err(|e| format!("script.json 형식이 잘못됐습니다({e})."))?;
    v.as_object_mut().ok_or("script.json이 객체가 아닙니다.")?.insert(key.into(), Value::Bool(value));
    crate::storage::save(&path, &v)
}

/// 파일 이름에서 경로 글자를 지운다 (spec 6.3 saveFile)
pub fn safe_file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = base.chars().map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { '_' } else { c }).collect();
    let clean = clean.trim().trim_matches('.').to_string();
    if clean.is_empty() {
        "파일".into()
    } else {
        clean.chars().take(150).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn patterns() {
        assert!(pattern_matches("https://www.chess.com/*", "https://www.chess.com/play/computer"));
        assert!(!pattern_matches("https://www.chess.com/*", "https://chess.com/"));
        assert!(!pattern_matches("https://www.chess.com/*", "https://evil.example/?https://www.chess.com/"));
        assert!(pattern_matches("https://images.chesscomfiles.com/*/wp.png", "https://images.chesscomfiles.com/a/b/wp.png"));
        assert!(!pattern_matches("https://images.chesscomfiles.com/*/wp.png", "https://images.chesscomfiles.com/a/wp.png.evil"));
        assert!(pattern_matches("*", "anything"));
        assert!(pattern_matches("https://a/*/x/*", "https://a/1/x/2"));
        assert!(pattern_matches("exact", "exact"));
        assert!(!pattern_matches("exact", "exact2"));
    }

    #[test]
    fn files_must_stay_inside() {
        let d = Path::new("C:\\scripts\\a");
        assert!(inside(d, "main.js").is_ok());
        assert!(inside(d, "pieces/wp.png").is_ok());
        assert!(inside(d, "../b/main.js").is_err());
        assert!(inside(d, "C:\\Windows\\win.ini").is_err());
        assert!(inside(d, "").is_err());
    }

    #[test]
    fn safe_names() {
        assert_eq!(safe_file_name("..\\..\\Windows\\evil.png"), "evil.png");
        assert_eq!(safe_file_name("a:b?.png"), "a_b_.png");
        assert_eq!(safe_file_name("..."), "파일");
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tbs-scripts-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn load_and_toggle() {
        let root = temp("load");
        let dir = root.join("demo");
        std::fs::create_dir_all(dir.join("img")).unwrap();
        std::fs::write(
            dir.join("script.json"),
            r#"{ "id": "demo", "name": "데모", "matches": ["https://example.com/*"], "run_at": "document_start",
                 "js": ["main.js"], "css": ["style.css"], "resource_overrides": [{ "pattern": "https://cdn.example/*/a.png", "file": "img/a.png" }],
                 "permissions": ["saveFile"], "enabled": true, "pinned": false, "extra": 1 }"#,
        )
        .unwrap();
        std::fs::write(dir.join("main.js"), "console.log('hi');").unwrap();
        std::fs::write(dir.join("style.css"), "/* url(\"tbs-file:...\") 처럼 쓴다 */ body { background: url(\"tbs-file:img/a.png\") }").unwrap();
        std::fs::write(dir.join("img/a.png"), b"PNG").unwrap();
        let s = load_all(&root);
        assert_eq!(s.len(), 1);
        assert!(s[0].error.is_none(), "{:?}", s[0].error);
        let code = s[0].code.as_ref().unwrap();
        assert!(code.contains("console.log('hi');"));
        assert!(code.contains("data:image/png;base64,UE5H"));
        assert_eq!(s[0].overrides[0].1, b"PNG");
        assert_eq!(s[0].manifest.permissions, vec![Permission::SaveFile]);
        set_flag(&dir, "enabled", false).unwrap();
        let again = load_one(&dir);
        assert!(!again.manifest.enabled);
        // 모르는 항목은 지우지 않는다
        assert!(std::fs::read_to_string(dir.join("script.json")).unwrap().contains("extra"));
    }

    #[test]
    fn keys_and_values() {
        assert_eq!(parse_keys("Alt+P").unwrap(), Keys { vk: 'P' as u32, ctrl: false, shift: false, alt: true });
        assert_eq!(parse_keys("ctrl + shift + y").unwrap(), Keys { vk: 'Y' as u32, ctrl: true, shift: true, alt: false });
        assert_eq!(parse_keys("F9").unwrap().vk, 0x78);
        assert!(parse_keys("P").is_err());
        assert!(parse_keys("Ctrl+T").is_err(), "브라우저 단축키와 겹친다");
        assert!(parse_keys("Hyper+P").is_err());
        let root = temp("values");
        let dir = root.join("v");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("script.json"),
            r#"{ "matches": ["*"], "permissions": ["tabs", "storage"], "shortcuts": [{ "id": "pip", "name": "PiP", "keys": "Alt+P" }],
                 "settings": [
                   { "key": "on", "type": "toggle", "label": "켜기", "default": true },
                   { "key": "px", "type": "number", "label": "거리", "default": 10, "min": 1, "max": 100 },
                   { "key": "act", "type": "select", "label": "동작", "default": "back", "options": [{ "value": "back", "label": "뒤로" }, { "value": "none", "label": "없음" }] },
                   { "key": "names", "type": "list", "label": "채널", "default": [] }
                 ] }"#,
        )
        .unwrap();
        let s = load_one(&dir);
        assert!(s.error.is_none(), "{:?}", s.error);
        assert!(s.needs_confirm() && s.all_sites());
        assert_eq!(s.values["px"], json!(10.0));
        assert!(s.code.as_ref().unwrap().contains("\"act\":\"back\""));
        assert!(set_value(&s, "px", &json!(500)).is_err());
        assert!(set_value(&s, "act", &json!("fly")).is_err());
        set_value(&s, "names", &json!([" 가 ", "가", "나"])).unwrap();
        let again = load_one(&dir);
        assert_eq!(again.values["names"], json!(["가", "나"]));
        // 저장 공간
        assert_eq!(storage_op(&dir, "get", Some("x"), None).unwrap(), Value::Null);
        storage_op(&dir, "set", Some("x"), Some(&json!({ "a": 1 }))).unwrap();
        assert_eq!(storage_op(&dir, "get", None, None).unwrap(), json!({ "x": { "a": 1 } }));
        storage_op(&dir, "remove", Some("x"), None).unwrap();
        assert!(storage_op(&dir, "drop", Some("x"), None).is_err());
    }

    /// 함께 넣는 예시가 모두 오류 없이 읽힌다
    #[test]
    fn examples_load() {
        let root = temp("examples");
        for (path, bytes) in crate::examples::FILES {
            let p = root.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, bytes).unwrap();
        }
        let all = load_all(&root);
        assert_eq!(all.len(), 5);
        for s in &all {
            assert!(s.error.is_none(), "{}: {:?}", s.manifest.id, s.error);
        }
        let g = all.iter().find(|s| s.manifest.id == "mouse-gestures").unwrap();
        assert!(g.needs_confirm() && g.fields.iter().all(|f| f.icon.as_deref().is_none_or(|i| i.starts_with("data:image/svg+xml"))));
    }

    #[test]
    fn broken_scripts_report_errors() {
        let root = temp("broken");
        std::fs::create_dir_all(root.join("bad")).unwrap();
        std::fs::write(root.join("bad/script.json"), "{ nope").unwrap();
        std::fs::create_dir_all(root.join("escape")).unwrap();
        std::fs::write(root.join("escape/script.json"), r#"{ "matches": ["*"], "js": ["../bad/script.json"] }"#).unwrap();
        let s = load_all(&root);
        assert_eq!(s.len(), 2);
        assert!(s.iter().all(|x| x.error.is_some() && x.code.is_none()));
    }
}
