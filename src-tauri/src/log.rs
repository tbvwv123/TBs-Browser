//! 오류 로그 파일 (spec 9장 8단계). 파일: %LOCALAPPDATA%\TBs Browser\logs\tbs.log
//! 설치판은 콘솔이 없어서 문제가 생기면 이 파일만 남는다. 1MB를 넘으면 tbs.old.log로 넘기고 새로 쓴다.

use std::{
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    sync::Mutex,
};

use crate::paths;

const MAX_BYTES: u64 = 1024 * 1024;
static LOCK: Mutex<()> = Mutex::new(());

pub fn dir() -> PathBuf {
    paths::data_root().join("logs")
}

fn file() -> PathBuf {
    dir().join("tbs.log")
}

/// "2026-11-10 14:03:22"
fn now() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

fn write(level: &str, msg: &str) {
    #[cfg(debug_assertions)]
    eprintln!("[tbs {level}] {msg}");
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let path = file();
    let _ = std::fs::create_dir_all(dir());
    if std::fs::metadata(&path).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        let _ = std::fs::rename(&path, dir().join("tbs.old.log"));
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{} {level} {}", now(), msg.replace('\n', "\n    "));
    }
}

pub fn info(msg: impl AsRef<str>) {
    write("INFO", msg.as_ref());
}

pub fn warn(msg: impl AsRef<str>) {
    write("WARN", msg.as_ref());
}

pub fn error(msg: impl AsRef<str>) {
    write("ERROR", msg.as_ref());
}

/// 멈춤(panic)도 로그에 남긴다
pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let at = info.location().map(|l| format!(" ({}:{})", l.file(), l.line())).unwrap_or_default();
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "알 수 없는 오류".into());
        error(format!("멈춤: {msg}{at}"));
        default(info);
    }));
}

/// 시작할 수 없을 때 사용자에게 알리는 창 (설치판에는 콘솔이 없다)
pub fn fatal_box(text: &str) {
    use windows::{
        core::HSTRING,
        Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK},
    };
    error(text);
    unsafe {
        MessageBoxW(None, &HSTRING::from(text), &HSTRING::from(crate::APP_NAME), MB_OK | MB_ICONERROR);
    }
}

/// WebView2 런타임이 있는지. 없으면 사용자에게 알리고 내려받는 페이지를 연다 (spec 5.7).
/// 설치 파일이 런타임을 설치하지만, 나중에 지워졌거나 설치 중 실패했을 수 있다.
pub fn check_webview2() -> bool {
    use webview2_com::Microsoft::Web::WebView2::Win32::GetAvailableCoreWebView2BrowserVersionString;
    use windows::core::PWSTR;
    let mut v = PWSTR::null();
    let ok = unsafe { GetAvailableCoreWebView2BrowserVersionString(None, &mut v) }.is_ok() && !v.is_null();
    let version = if ok { unsafe { v.to_string().unwrap_or_default() } } else { String::new() };
    if !version.is_empty() {
        info(format!("시작: {} {}, WebView2 {version}", crate::APP_NAME, env!("CARGO_PKG_VERSION")));
        return true;
    }
    fatal_box(
        "Microsoft Edge WebView2 런타임이 없어 시작할 수 없습니다.\n\n\
         확인을 누르면 내려받는 페이지가 열립니다. \"Evergreen 부트스트래퍼\"를 받아 설치한 뒤 TB's Browser를 다시 켜 주세요.",
    );
    let _ = std::process::Command::new("explorer").arg("https://go.microsoft.com/fwlink/p/?LinkId=2124703").spawn();
    false
}
