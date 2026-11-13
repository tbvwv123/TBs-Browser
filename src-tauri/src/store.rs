//! 스크립트 저장소 (spec 6.8). 공개 GitHub 저장소 하나의 `index.json`과 스크립트별 zip을 받는다.
//!
//! - 목록: `index.json`을 받아 둔다. 찾아보기(`tbs://scripts`)와 확장 패널의 "업데이트 있음"이 이것을 본다.
//! - 설치: 확인을 받으면 zip을 받아 `scripts\.installing-<id>`에 풀고, 오류 없이 읽히면 `scripts\<id>`로 옮긴다.
//! - 업데이트: 하루에 한 번 목록을 새로 받는다. 권한이 늘었으면 다시 묻는다. 설정 값(`settings.json`)과
//!   저장 공간(`storage.json`), 켜짐·고정은 그대로 두고, 옛 폴더는 휴지통으로 옮긴다.

use std::{
    path::PathBuf,
    sync::Mutex,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::AppHandle;
use windows::core::HSTRING;

use crate::{
    log,
    scripts::{self, Permission},
    site_scripts,
};

const BASE: &str = "https://raw.githubusercontent.com/tbvwv123/TBs-Browser-scripts/main/";
/// zip 하나의 크기 한도
const MAX_ZIP: u64 = 20 * 1024 * 1024;
const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// `index.json`의 한 줄
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub matches: Vec<String>,
    #[serde(default)]
    pub permissions: Vec<Permission>,
    /// 데이터 URL
    #[serde(default)]
    pub icon: Option<String>,
    /// 저장소 안의 zip 경로 ("dist/pip.zip")
    pub zip: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Deserialize)]
struct Index {
    #[allow(dead_code)]
    version: u32,
    scripts: Vec<Entry>,
}

static INDEX: Mutex<Option<Vec<Entry>>> = Mutex::new(None);
/// 확인을 기다리는 설치·업데이트 (스크립트 id)
static PENDING: Mutex<Option<String>> = Mutex::new(None);

/// "1.10.0" > "1.9.2". 숫자가 아닌 칸과 없는 칸은 0
pub fn newer(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> { s.split('.').map(|p| p.trim().parse().unwrap_or(0)).collect() };
    let (x, y) = (parse(a), parse(b));
    for i in 0..x.len().max(y.len()) {
        let (p, q) = (x.get(i).copied().unwrap_or(0), y.get(i).copied().unwrap_or(0));
        if p != q {
            return p > q;
        }
    }
    false
}

fn safe_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// 받기. Windows의 URLDownloadToFile을 쓴다 (시스템 프록시와 인증서). 캐시를 피하려고 시각을 붙인다
fn download(path: &str) -> Result<Vec<u8>, String> {
    let dir = site_scripts::dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!(".download-{}", crate::history::now_ms()));
    let url = format!("{BASE}{path}?t={}", crate::history::now_ms());
    let r = unsafe { windows::Win32::System::Com::Urlmon::URLDownloadToFileW(None, &HSTRING::from(url), &HSTRING::from(tmp.as_path()), 0, None) };
    let bytes = r.map_err(|e| format!("저장소에 연결할 수 없습니다({e}). 인터넷 연결을 확인해 주세요.")).and_then(|_| std::fs::read(&tmp).map_err(|e| e.to_string()));
    let _ = std::fs::remove_file(&tmp);
    bytes
}

/// 목록을 새로 받는다
pub fn fetch() -> Result<Vec<Entry>, String> {
    let bytes = download("index.json")?;
    let index: Index = serde_json::from_slice(&bytes).map_err(|e| format!("저장소 목록을 읽지 못했습니다({e}). 잠시 뒤 다시 시도해 주세요."))?;
    let list: Vec<Entry> = index
        .scripts
        .into_iter()
        .filter(|e| safe_id(&e.id) && e.zip.starts_with("dist/") && !e.zip.contains("..") && e.zip.ends_with(".zip"))
        .collect();
    *INDEX.lock().unwrap() = Some(list.clone());
    Ok(list)
}

/// 받아 둔 목록 (없으면 받는다)
pub fn entries(refresh: bool) -> Result<Vec<Entry>, String> {
    if !refresh {
        if let Some(l) = INDEX.lock().unwrap().clone() {
            return Ok(l);
        }
    }
    fetch()
}

/// 설치한 것보다 새 판이 저장소에 있으면 그 판
pub fn update_for(id: &str, installed: &str) -> Option<String> {
    INDEX.lock().unwrap().as_ref()?.iter().find(|e| e.id == id && newer(&e.version, installed)).map(|e| e.version.clone())
}

/// 시작할 때와 하루에 한 번 목록을 받아, 업데이트가 있으면 확장 패널에 알린다
pub fn start(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || loop {
        match fetch() {
            Ok(_) => {
                let a = app.clone();
                let _ = app.run_on_main_thread(move || site_scripts::reload(&a));
            }
            Err(e) => log::warn(format!("스크립트 저장소 목록을 받지 못함: {e}")),
        }
        std::thread::sleep(DAY);
    });
}

/// "설치"나 "업데이트"를 눌렀다. 설치는 늘 묻고, 업데이트는 권한이 늘었을 때만 묻는다 (시안 M)
pub fn begin(app: &AppHandle, id: &str) -> Result<(), String> {
    let entry = entries(false)?.into_iter().find(|e| e.id == id).ok_or("저장소에 그 스크립트가 없습니다. 찾아보기를 새로 열어 주세요.")?;
    let installed = site_scripts::with_script(app, id, |s| (s.manifest.version.clone(), s.manifest.permissions.clone(), s.all_sites(), s.manifest.matches.clone()));
    let (mode, from, new_perms, sites_changed) = match &installed {
        None => ("install", None, Vec::new(), true),
        Some((ver, perms, was_all, old_matches)) => {
            let added: Vec<Permission> = entry.permissions.iter().filter(|p| !perms.contains(p)).copied().collect();
            let wider = scripts::all_sites(&entry.matches) && !was_all;
            if added.is_empty() && !wider {
                // 권한이 그대로인 업데이트는 묻지 않는다
                let (a, e) = (app.clone(), entry.clone());
                std::thread::spawn(move || finish(&a, e));
                return Ok(());
            }
            let changed = scripts::site_names(&entry.matches) != scripts::site_names(old_matches) || wider;
            ("update", Some(ver.clone()), added, changed)
        }
    };
    *PENDING.lock().unwrap() = Some(id.to_string());
    let info = json!({
        "kind": "script", "mode": mode,
        "id": entry.id, "name": entry.name, "version": entry.version, "description": entry.description, "author": entry.author,
        "enabled": false, "pinned": false, "icon": entry.icon, "popup": null, "options": null,
        "matches": entry.matches, "allSites": scripts::all_sites(&entry.matches), "sites": scripts::site_names(&entry.matches),
        "permissions": entry.permissions.iter().map(|p| p.sentence()).collect::<Vec<_>>(),
        "newPermissions": new_perms.iter().map(|p| p.sentence()).collect::<Vec<_>>(),
        "sitesChanged": sites_changed,
        "error": null, "updateFrom": from,
    });
    crate::browser::emit_ui(app, "install_ready", info);
    Ok(())
}

/// 확인 창의 답
pub fn confirm(app: &AppHandle, id: &str, accept: bool) {
    let pending = PENDING.lock().unwrap().take();
    if !accept || pending.as_deref() != Some(id) {
        return;
    }
    let Some(entry) = INDEX.lock().unwrap().as_ref().and_then(|l| l.iter().find(|e| e.id == id).cloned()) else { return };
    let app = app.clone();
    std::thread::spawn(move || finish(&app, entry));
}

/// 받아서 설치하고, 끝나면 목록을 다시 읽는다. 실패하면 알린다
fn finish(app: &AppHandle, entry: Entry) {
    let installed_dir = site_scripts::with_script(app, &entry.id, |s| (s.dir.clone(), s.manifest.enabled, s.manifest.pinned));
    let result = install(&entry, installed_dir);
    let a = app.clone();
    let name = entry.name.clone();
    let _ = app.run_on_main_thread(move || {
        site_scripts::reload(&a);
        crate::browser::reapply_scripts(&a, false);
        match result {
            Ok(()) => log::info(format!("스크립트 {name} 설치함")),
            Err(e) => {
                let msg = format!("{name}을(를) 설치하지 못했습니다: {e}");
                log::warn(&msg);
                crate::browser::emit_ui(&a, "notice", msg);
            }
        }
        crate::browser::emit_ui(&a, "store_done", ());
    });
}

fn install(entry: &Entry, old: Option<(PathBuf, bool, bool)>) -> Result<(), String> {
    let root = site_scripts::dir();
    let bytes = download(&entry.zip)?;
    if bytes.len() as u64 > MAX_ZIP || (entry.size > 0 && bytes.len() as u64 != entry.size) {
        return Err("받은 파일의 크기가 목록과 다릅니다. 잠시 뒤 다시 시도해 주세요.".into());
    }
    let tmp = root.join(format!(".installing-{}", entry.id));
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).map_err(|e| format!("임시 폴더를 비우지 못했습니다({e})."))?;
    }
    let zip_path = root.join(format!(".download-{}.zip", entry.id));
    std::fs::write(&zip_path, &bytes).map_err(|e| e.to_string())?;
    let unzipped = crate::extensions::unzip(&zip_path, &tmp);
    let _ = std::fs::remove_file(&zip_path);
    unzipped?;
    let fail = |e: String| {
        let _ = std::fs::remove_dir_all(&tmp);
        Err(e)
    };
    let loaded = scripts::load_one(&tmp);
    if let Some(e) = loaded.error {
        return fail(format!("스크립트 파일에 문제가 있습니다({e})"));
    }
    if loaded.manifest.id != entry.id {
        return fail("목록과 스크립트의 id가 다릅니다.".into());
    }
    match old {
        Some((dir, enabled, pinned)) => {
            // 사용자가 만든 값은 새 판으로 옮긴다
            for f in ["settings.json", "storage.json"] {
                if dir.join(f).exists() {
                    if let Err(e) = std::fs::copy(dir.join(f), tmp.join(f)) {
                        return fail(format!("{f}을(를) 옮기지 못했습니다({e})."));
                    }
                }
            }
            scripts::set_flag(&tmp, "enabled", enabled)?;
            scripts::set_flag(&tmp, "pinned", pinned)?;
            if let Err(e) = site_scripts::to_recycle_bin(&dir) {
                return fail(e);
            }
        }
        None => scripts::set_flag(&tmp, "enabled", true)?,
    }
    let target = root.join(&entry.id);
    if target.exists() {
        return fail(format!("\"{}\" 폴더가 이미 있습니다. 스크립트 폴더에서 그 폴더를 옮긴 뒤 다시 해 주세요.", entry.id));
    }
    std::fs::rename(&tmp, &target).map_err(|e| format!("스크립트 폴더를 옮기지 못했습니다({e})."))
}

/// 찾아보기 목록. 상태는 UI가 설치한 스크립트와 견줘 정한다
pub fn list_json(refresh: bool) -> Result<Vec<Value>, String> {
    Ok(entries(refresh)?.into_iter().map(|e| serde_json::to_value(e).unwrap_or(Value::Null)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(newer("1.10.0", "1.9.2"));
        assert!(newer("1.0.1", "1.0"));
        assert!(!newer("1.0.0", "1.0"));
        assert!(newer("1.0.0", ""));
        assert!(!newer("0.9", "1.0.0"));
        assert!(safe_id("youtube-block") && !safe_id("../x") && !safe_id("A"));
    }
}
