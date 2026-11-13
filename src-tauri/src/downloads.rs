//! 다운로드 (spec 5.4). WebView2의 기본 다운로드 창 대신 호스트가 받는다.
//! 진행 중인 것만 툴바에 진행 고리로 보이고, 전체 목록은 다운로드 페이지에서 본다.
//! 이 파일의 함수는 메인 스레드에서만 부른다.

use std::{cell::RefCell, collections::HashMap, path::PathBuf};

use tauri::{AppHandle, Manager};
use webview2_com::{Microsoft::Web::WebView2::Win32::*, *};
use windows::core::{Interface, HSTRING, PWSTR};

use crate::{
    state::TabId,
    history::{Download, History},
    settings,
};

thread_local! {
    /// 진행 중인 다운로드 (기록 id → WebView2 다운로드 객체)
    static ACTIVE: RefCell<HashMap<i64, (ICoreWebView2DownloadOperation, TabId)>> = RefCell::new(HashMap::new());
}

const RECENT: u32 = 200;

pub fn list(app: &AppHandle) -> Vec<Download> {
    app.state::<History>().downloads(RECENT).unwrap_or_default()
}

fn emit_list(app: &AppHandle) {
    crate::browser::emit_ui(app, "downloads_changed", list(app));
    crate::popup::refresh_downloads(app);
}

/// 같은 이름이 있으면 "이름 (1).확장자"처럼 비어 있는 이름을 찾는다
pub fn free_path(dir: &std::path::Path, name: &str) -> PathBuf {
    let clean: String = name.chars().map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { '_' } else { c }).collect();
    let clean = if clean.trim().is_empty() { "download".to_string() } else { clean };
    let p = dir.join(&clean);
    if !p.exists() {
        return p;
    }
    let (stem, ext) = match clean.rfind('.') {
        Some(i) if i > 0 => (&clean[..i], &clean[i..]),
        _ => (clean.as_str(), ""),
    };
    (1..10_000).map(|n| dir.join(format!("{stem} ({n}){ext}"))).find(|p| !p.exists()).unwrap_or(p)
}

/// 콘텐츠 웹뷰에 다운로드 처리기를 건다. 돌려준 토큰은 웹뷰를 닫을 때 뗀다.
/// 이 탭에서 받는 중인 다운로드가 있는지 (있으면 재우거나 버리지 않는다, spec 7장)
pub fn tab_busy(tab: TabId) -> bool {
    ACTIVE.with(|a| a.borrow().values().any(|(_, t)| *t == tab))
}

/// 받는 중인 다운로드를 연 탭의 (이름, 탭 id). 개발용 `orphans` 확인에 쓴다
#[cfg(debug_assertions)]
pub fn tab_entries() -> Vec<(&'static str, TabId)> {
    ACTIVE.with(|a| a.borrow().values().map(|(_, t)| ("downloads.ACTIVE", *t)).collect())
}

pub fn attach(app: &AppHandle, core: &ICoreWebView2, tab: TabId) -> Option<i64> {
    let c4 = core.cast::<ICoreWebView2_4>().ok()?;
    let app = app.clone();
    let mut token = 0i64;
    let handler = DownloadStartingEventHandler::create(Box::new(move |_, args| unsafe {
        let Some(args) = args else { return Ok(()) };
        let op = args.DownloadOperation()?;
        // 기본 다운로드 창은 띄우지 않고, 설정의 다운로드 폴더에 저장한다
        args.SetHandled(true)?;
        let mut suggested = PWSTR::null();
        args.ResultFilePath(&mut suggested)?;
        let suggested = PathBuf::from(take_pwstr(suggested));
        let name = suggested.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let dir = settings::get(&app).download_dir();
        let _ = std::fs::create_dir_all(&dir);
        let target = free_path(&dir, &name);
        args.SetResultFilePath(&HSTRING::from(target.as_path()))?;

        let mut uri = PWSTR::null();
        op.Uri(&mut uri)?;
        let mut total = -1i64;
        let _ = op.TotalBytesToReceive(&mut total);
        let hist = app.state::<History>();
        let Ok(id) = hist.add_download(&take_pwstr(uri), &target.to_string_lossy(), total) else { return Ok(()) };
        ACTIVE.with(|a| a.borrow_mut().insert(id, (op.clone(), tab)));

        let app2 = app.clone();
        let mut t = 0i64;
        op.add_BytesReceivedChanged(
            &BytesReceivedChangedEventHandler::create(Box::new(move |op, _| {
                if let Some(op) = op {
                    progress(&app2, id, &op, false);
                }
                Ok(())
            })),
            &mut t,
        )?;
        let app3 = app.clone();
        op.add_StateChanged(
            &StateChangedEventHandler::create(Box::new(move |op, _| {
                if let Some(op) = op {
                    progress(&app3, id, &op, true);
                }
                Ok(())
            })),
            &mut t,
        )?;
        emit_list(&app);
        Ok(())
    }));
    unsafe { c4.add_DownloadStarting(&handler, &mut token) }.ok()?;
    Some(token)
}

/// 사이트 스크립트가 저장한 파일도 다운로드 목록에 남긴다 (spec 6.3)
pub fn record_saved(app: &AppHandle, page: &str, path: &std::path::Path, size: i64) {
    let hist = app.state::<History>();
    if let Ok(id) = hist.add_download(page, &path.to_string_lossy(), size) {
        let _ = hist.update_download(id, "done", size, size, &path.to_string_lossy());
    }
    emit_list(app);
}

pub fn detach(core: &ICoreWebView2, token: i64) {
    if let Ok(c4) = core.cast::<ICoreWebView2_4>() {
        let _ = unsafe { c4.remove_DownloadStarting(token) };
    }
}

fn progress(app: &AppHandle, id: i64, op: &ICoreWebView2DownloadOperation, state_changed: bool) {
    let (mut received, mut total) = (0i64, -1i64);
    let mut st = COREWEBVIEW2_DOWNLOAD_STATE::default();
    let mut path = PWSTR::null();
    unsafe {
        let _ = op.BytesReceived(&mut received);
        let _ = op.TotalBytesToReceive(&mut total);
        let _ = op.State(&mut st);
        let _ = op.ResultFilePath(&mut path);
    }
    let state = match st {
        COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED => "done",
        COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED => {
            let mut reason = COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON::default();
            let _ = unsafe { op.InterruptReason(&mut reason) };
            if reason == COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_CANCELED {
                "cancelled"
            } else {
                crate::log::warn(format!("다운로드 {id} 실패, 이유 코드 {}", reason.0));
                "failed"
            }
        }
        _ => "progress",
    };
    let hist = app.state::<History>();
    let _ = hist.update_download(id, state, received, total, &take_pwstr(path));
    if state != "progress" {
        ACTIVE.with(|a| a.borrow_mut().remove(&id));
    }
    if state_changed || state != "progress" {
        emit_list(app);
    } else if let Some(d) = hist.download(id) {
        crate::browser::emit_ui(app, "download_progress", &d);
        crate::popup::refresh_downloads(app);
    }
}

pub fn cancel(app: &AppHandle, id: i64) {
    if let Some((op, _)) = ACTIVE.with(|a| a.borrow().get(&id).cloned()) {
        let _ = unsafe { op.Cancel() };
        return;
    }
    // 이번 실행에서 받는 중이 아닌 항목: 기록이 "받는 중"으로 남아 있을 때만 취소로 바꾼다
    let hist = app.state::<History>();
    if let Some(d) = hist.download(id).filter(|d| d.state == "progress") {
        let _ = hist.update_download(id, "cancelled", d.received, d.total, &d.path);
        emit_list(app);
    }
}

/// 파일 열기
pub fn open_file(app: &AppHandle, id: i64) -> std::result::Result<(), String> {
    let d = app.state::<History>().download(id).ok_or("다운로드 기록을 찾지 못했습니다.")?;
    if !std::path::Path::new(&d.path).exists() {
        return Err(format!("파일이 없습니다({}). 옮겼거나 지웠을 수 있습니다.", d.path));
    }
    shell_open(&d.path)
}

/// 폴더 열기: 탐색기에서 그 파일을 고른 상태로 연다
pub fn show_in_folder(app: &AppHandle, id: i64) -> std::result::Result<(), String> {
    let d = app.state::<History>().download(id).ok_or("다운로드 기록을 찾지 못했습니다.")?;
    let p = std::path::Path::new(&d.path);
    if p.exists() {
        std::process::Command::new("explorer.exe")
            .arg(format!("/select,{}", d.path))
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("폴더를 열지 못했습니다({e})."))
    } else {
        let dir = p.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
        shell_open(&dir)
    }
}

fn shell_open(path: &str) -> std::result::Result<(), String> {
    use windows::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
    let r = unsafe { ShellExecuteW(None, &HSTRING::from("open"), &HSTRING::from(path), None, None, SW_SHOWNORMAL) };
    if r.0 as isize > 32 {
        Ok(())
    } else {
        Err(format!("열지 못했습니다({path}). 이 파일을 열 프로그램이 있는지 확인해 주세요."))
    }
}

#[cfg(test)]
mod tests {
    use super::free_path;

    #[test]
    fn file_names_are_cleaned_and_not_overwritten() {
        let dir = std::env::temp_dir().join(format!("tbs-dl-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(free_path(&dir, "a.zip"), dir.join("a.zip"));
        std::fs::write(dir.join("a.zip"), b"x").unwrap();
        assert_eq!(free_path(&dir, "a.zip"), dir.join("a (1).zip"));
        assert_eq!(free_path(&dir, "..\\evil:name?.txt"), dir.join(".._evil_name_.txt"));
    }
}
