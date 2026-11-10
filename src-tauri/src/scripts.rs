//! 사이트 스크립트 (spec 6장). 확장 프로그램 대신 사이트별로 스크립트와 스타일을 끼워 넣는다.
//! 폴더: %LOCALAPPDATA%\TBs Browser\scripts\<id>\script.json

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

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

/// 스크립트가 브라우저에 부탁할 수 있는 일 (spec 6.3). 이 셋이 전부다.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Permission {
    Screenshot,
    SaveFile,
    CopyText,
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
        }
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
fn build_code(m: &Manifest, js: &str, css: &str) -> String {
    let id = serde_json::to_string(&m.id).unwrap();
    let patterns = serde_json::to_string(&m.matches).unwrap();
    let css = serde_json::to_string(css).unwrap();
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
// 브라우저에 부탁하기 (spec 6.3): tbs.request("screenshot" | "saveFile" | "copyText", 값)
const pending = new Map();
let seq = 0;
if (window.chrome && window.chrome.webview) {{
  window.chrome.webview.addEventListener("message", e => {{
    const r = e.data && e.data.__tbs_site_script_reply;
    if (!r || r.script !== SCRIPT_ID || !pending.has(r.rid)) return;
    const p = pending.get(r.rid); pending.delete(r.rid);
    r.ok ? p.resolve(r.result) : p.reject(new Error(r.error));
  }});
}}
const tbs = Object.freeze({{
  request(kind, data) {{
    return new Promise((resolve, reject) => {{
      if (!(window.chrome && window.chrome.webview)) return reject(new Error("브라우저에 부탁할 수 없는 페이지입니다."));
      const rid = ++seq;
      pending.set(rid, {{ resolve, reject }});
      window.chrome.webview.postMessage(JSON.stringify({{ __tbs_site_script: {{ script: SCRIPT_ID, rid, kind, data: data || {{}} }} }}));
    }});
  }}
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
    let fail = |m: Manifest, e: String| Script { manifest: m, dir: dir.to_path_buf(), code: None, overrides: Vec::new(), error: Some(e) };
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
    let result = (|| -> Result<(String, Vec<(String, Vec<u8>, &'static str)>), String> {
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
        Ok((build_code(&m, &js, &css), overrides))
    })();
    match result {
        Ok((code, overrides)) => Script { manifest: m, dir: dir.to_path_buf(), code: Some(code), overrides, error: None },
        Err(e) => fail(m, e),
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
