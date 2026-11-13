//! 설정 (spec 5.4). 파일: state\settings.json

use std::{collections::BTreeMap, path::PathBuf, sync::Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::{paths, storage};

pub const VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Light,
    Dark,
    System,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum SearchEngine {
    #[default]
    Google,
    Naver,
    Bing,
    Duckduckgo,
}

impl SearchEngine {
    /// (검색 주소, 검색어 매개변수 이름)
    pub fn endpoint(self) -> (&'static str, &'static str) {
        match self {
            SearchEngine::Google => ("https://www.google.com/search", "q"),
            SearchEngine::Naver => ("https://search.naver.com/search.naver", "query"),
            SearchEngine::Bing => ("https://www.bing.com/search", "q"),
            SearchEngine::Duckduckgo => ("https://duckduckgo.com/", "q"),
        }
    }
}

/// 광고 차단 설정 (spec 6.7)
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct AdblockSettings {
    pub enabled: bool,
    /// 목록 ID("easylist" 등) → 켜짐. 없으면 켜짐
    pub lists: BTreeMap<String, bool>,
    /// 광고 차단을 끈 사이트 ("youtube.com")
    pub disabled_sites: Vec<String>,
}

impl Default for AdblockSettings {
    fn default() -> Self {
        AdblockSettings { enabled: true, lists: BTreeMap::new(), disabled_sites: Vec::new() }
    }
}

/// 메모리 정책 값 (spec 7장). 6단계에서 적용한다.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct MemoryPolicy {
    pub suspend_after_minutes: u32,
    pub discard_after_minutes: u32,
    pub max_awake_tabs: u32,
}

impl Default for MemoryPolicy {
    fn default() -> Self {
        MemoryPolicy { suspend_after_minutes: 5, discard_after_minutes: 30, max_awake_tabs: 12 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub version: u32,
    pub theme: Theme,
    pub search_engine: SearchEngine,
    /// 비어 있으면 Windows의 "다운로드" 폴더
    pub download_dir: String,
    /// 시작할 때 이전 탭을 복원할지
    pub restore_tabs: bool,
    pub memory: MemoryPolicy,
    /// 사이트별 권한 (spec 5.5, 8장). 사이트("https://host") → 권한 이름("camera" 등) → "allow" | "block"
    pub permissions: BTreeMap<String, BTreeMap<String, String>>,
    pub adblock: AdblockSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: VERSION,
            theme: Theme::Light,
            search_engine: SearchEngine::Google,
            download_dir: String::new(),
            restore_tabs: true,
            memory: MemoryPolicy::default(),
            permissions: BTreeMap::new(),
            adblock: AdblockSettings::default(),
        }
    }
}

impl Settings {
    pub fn download_dir(&self) -> PathBuf {
        if !self.download_dir.trim().is_empty() {
            // WebView2는 '/'가 섞인 저장 경로를 받지 않는다
            return PathBuf::from(self.download_dir.trim().replace('/', "\\"));
        }
        std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("Downloads")).unwrap_or_else(std::env::temp_dir)
    }
}

#[derive(Default)]
pub struct SettingsStore(Mutex<Settings>);

fn file() -> PathBuf {
    paths::state_dir().join("settings.json")
}

/// 읽지 못했을 때의 알림을 돌려준다
pub fn load(app: &AppHandle) -> Option<String> {
    let (s, notice) = match storage::load::<Settings>(&file()) {
        Ok(Some(s)) => (Settings { version: VERSION, ..s }, None),
        Ok(None) => (Settings::default(), None),
        Err(msg) => (Settings::default(), Some(msg)),
    };
    *app.state::<SettingsStore>().0.lock().unwrap() = s;
    notice
}

pub fn get(app: &AppHandle) -> Settings {
    app.state::<SettingsStore>().0.lock().unwrap().clone()
}

/// 설정이 바뀐 것을 UI에 다시 알린다 (목록 갱신처럼 설정 밖의 값이 바뀌었을 때)
pub fn notify(app: &AppHandle) {
    let s = get(app);
    crate::browser::emit_ui(app, "settings_changed", &s);
}

pub fn set(app: &AppHandle, mut s: Settings) -> Result<Settings, String> {
    s.version = VERSION;
    s.memory.max_awake_tabs = s.memory.max_awake_tabs.max(1);
    s.permissions.retain(|_, m| !m.is_empty());
    s.adblock.disabled_sites.sort();
    s.adblock.disabled_sites.dedup();
    *app.state::<SettingsStore>().0.lock().unwrap() = s.clone();
    crate::browser::emit_ui(app, "settings_changed", &s);
    storage::save(&file(), &s)?;
    Ok(s)
}

/// 사이트에 기억해 둔 권한. 허용이면 Some(true), 차단이면 Some(false), 묻지 않았으면 None.
pub fn permission(app: &AppHandle, site: &str, kind: &str) -> Option<bool> {
    let s = app.state::<SettingsStore>();
    let s = s.0.lock().unwrap();
    s.permissions.get(site).and_then(|m| m.get(kind)).map(|v| v == "allow")
}

/// 사용자가 고른 권한을 기억한다
pub fn remember_permission(app: &AppHandle, site: &str, kind: &str, allow: bool) -> Result<(), String> {
    let mut s = get(app);
    s.permissions.entry(site.to_string()).or_default().insert(kind.to_string(), if allow { "allow" } else { "block" }.to_string());
    set(app, s).map(|_| ())
}

/// 폴더 고르기 창 (다운로드 폴더). 메인 스레드에서 부른다.
pub fn pick_folder(owner: Option<isize>, start: &std::path::Path) -> Option<String> {
    use windows::{
        core::HSTRING,
        Win32::{
            Foundation::HWND,
            System::Com::{CoCreateInstance, CLSCTX_ALL},
            UI::Shell::{FileOpenDialog, IFileOpenDialog, IShellItem, SHCreateItemFromParsingName, FOS_PICKFOLDERS, SIGDN_FILESYSPATH},
        },
    };
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_ALL).ok()?;
        let _ = dialog.SetOptions(dialog.GetOptions().ok()? | FOS_PICKFOLDERS);
        let _ = dialog.SetTitle(&HSTRING::from("다운로드 폴더 고르기"));
        if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(start), None) {
            let _ = dialog.SetFolder(&item);
        }
        dialog.Show(owner.map(|h| HWND(h as _))).ok()?;
        let item = dialog.GetResult().ok()?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = name.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(name.0 as _));
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_spec() {
        let s = Settings::default();
        assert_eq!(s.theme, Theme::Light);
        assert_eq!(s.search_engine, SearchEngine::Google);
        assert_eq!(s.memory, MemoryPolicy { suspend_after_minutes: 5, discard_after_minutes: 30, max_awake_tabs: 12 });
    }

    #[test]
    fn missing_fields_use_defaults() {
        let s: Settings = serde_json::from_str(r#"{ "version": 1, "theme": "dark" }"#).unwrap();
        assert_eq!(s.theme, Theme::Dark);
        assert!(s.restore_tabs);
    }
}
