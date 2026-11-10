//! 개발할 때만 쓰는 기능. 릴리스 빌드에는 들어가지 않는다.
//!
//! - `TBS_DEV_DESKTOP=<가상 데스크톱 GUID>`: 창을 그 데스크톱에 포커스 없이 띄운다.
//!   사용자가 쓰는 데스크톱을 건드리지 않고 시험하기 위해서다.
//! - `TBS_DEV_HOOK=<폴더>`: 그 폴더의 `cmd.txt`에 적힌 시험 명령을 실행하고 `cmd-done.txt`에 결과를 쓴다.

use tauri::AppHandle;

/// 포커스를 가져가면 안 되는 실행인지 (다른 데스크톱에서 시험 중).
pub fn quiet() -> bool {
    cfg!(debug_assertions) && std::env::var_os("TBS_DEV_DESKTOP").is_some()
}

/// 시험용: 크롬 확장 허용을 켤지 (TBS_DEV_EXTENSIONS). 모든 웹뷰가 같은 값을 써야 한다 (0단계 13번).
pub fn extensions() -> bool {
    cfg!(debug_assertions) && std::env::var_os("TBS_DEV_EXTENSIONS").is_some()
}

/// 개발용 데스크톱이 정해져 있으면 창을 그리로 옮긴다 (보이기 전에 부른다)
#[allow(unused_variables)]
pub fn move_to_dev_desktop(hwnd: isize) {
    #[cfg(debug_assertions)]
    if let Ok(guid) = std::env::var("TBS_DEV_DESKTOP") {
        unsafe {
            use windows::Win32::{
                Foundation::{E_INVALIDARG, HWND},
                System::Com::{CoCreateInstance, CLSCTX_ALL},
                UI::Shell::{IVirtualDesktopManager, VirtualDesktopManager},
            };
            let r = (|| -> windows::core::Result<()> {
                let g = windows::core::GUID::try_from(guid.as_str()).map_err(|_| windows::core::Error::from(E_INVALIDARG))?;
                let m: IVirtualDesktopManager = CoCreateInstance(&VirtualDesktopManager, None, CLSCTX_ALL)?;
                m.MoveWindowToDesktop(HWND(hwnd as _), &g)
            })();
            eprintln!("[tbs dev] 개발용 데스크톱으로 옮김: {r:?}");
        }
    }
}

/// 숨겨 만든 창을 보인다. 개발용 데스크톱이 정해져 있으면 그리로 옮기고 활성화 없이 보인다.
pub fn show_main_window(window: &tauri::Window) {
    if quiet() {
        if let Ok(hwnd) = window.hwnd() {
            move_to_dev_desktop(hwnd.0 as isize);
            unsafe {
                use windows::Win32::{
                    Foundation::HWND,
                    UI::WindowsAndMessaging::{ShowWindow, SW_SHOWNA},
                };
                let _ = ShowWindow(HWND(hwnd.0), SW_SHOWNA);
            }
            return;
        }
    }
    let _ = window.show();
    let _ = window.set_focus();
}

#[cfg(not(debug_assertions))]
pub fn start_hook(_app: &AppHandle) {}

#[cfg(debug_assertions)]
pub fn start_hook(app: &AppHandle) {
    let Some(dir) = std::env::var_os("TBS_DEV_HOOK").map(std::path::PathBuf::from) else { return };
    let _ = std::fs::create_dir_all(&dir);
    let app = app.clone();
    std::thread::spawn(move || hook_loop(app, dir));
}

#[cfg(debug_assertions)]
fn hook_loop(app: AppHandle, dir: std::path::PathBuf) {
    use std::{sync::mpsc::channel, time::Duration};
    use tauri::Manager;
    use webview2_com::{Microsoft::Web::WebView2::Win32::*, *};
    use windows::core::HSTRING;

    let cmd_file = dir.join("cmd.txt");
    let done_file = dir.join("cmd-done.txt");
    loop {
        std::thread::sleep(Duration::from_millis(150));
        let Ok(cmd) = std::fs::read_to_string(&cmd_file) else { continue };
        let _ = std::fs::remove_file(&cmd_file);
        let cmd = cmd.trim().to_string();
        let mut parts = cmd.splitn(3, ' ');
        let (verb, label, rest) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or("").to_string());
        let (tx, rx) = channel::<String>();
        let result = match (verb, app.get_webview(label)) {
            ("state", _) => serde_json::to_string(&crate::browser::snapshot(&app)).unwrap_or_default(),
            ("labels", _) => format!("{:?}", app.webviews().keys().collect::<Vec<_>>()),
            // 메모리 상태 (spec 7장): 탭마다 Awake / Background / Suspended. 버린 탭은 빠진다.
            ("mem", _) => {
                let _ = app.run_on_main_thread(move || {
                    let mut v: Vec<_> = crate::memory::phases().into_iter().map(|(id, p)| format!("{id}:{p:?}")).collect();
                    v.sort();
                    let _ = tx.send(v.join(" "));
                });
                rx.recv_timeout(Duration::from_secs(5)).unwrap_or_else(|_| "timeout".into())
            }
            ("js", Some(wv)) => {
                let _ = wv.with_webview(move |p| unsafe {
                    let Ok(core) = p.controller().CoreWebView2() else { return };
                    let h = ExecuteScriptCompletedHandler::create(Box::new(move |err, res| {
                        let _ = tx.send(if err.is_ok() { res } else { format!("ERR {err:?}") });
                        Ok(())
                    }));
                    let _ = core.ExecuteScript(&HSTRING::from(rest), &h);
                });
                rx.recv_timeout(Duration::from_secs(15)).unwrap_or_else(|_| "timeout".into())
            }
            // 사용자가 누른 것처럼 스크립트를 돌린다 (전체 화면 요청처럼 사용자 동작이 있어야 되는 것을 시험할 때)
            ("gesture", Some(wv)) => {
                let _ = wv.with_webview(move |p| unsafe {
                    let Ok(core) = p.controller().CoreWebView2() else { return };
                    let params = serde_json::json!({ "expression": rest, "userGesture": true, "awaitPromise": true, "returnByValue": true }).to_string();
                    let h = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |err, res| {
                        let _ = tx.send(if err.is_ok() { res } else { format!("ERR {err:?}") });
                        Ok(())
                    }));
                    let _ = core.CallDevToolsProtocolMethod(&HSTRING::from("Runtime.evaluate"), &HSTRING::from(params), &h);
                });
                rx.recv_timeout(Duration::from_secs(15)).unwrap_or_else(|_| "timeout".into())
            }
            // 페이지가 전체 화면을 켠 것처럼 한다: "fs <탭 id> on|off" (창이 앞에 없으면 Chromium이 실제 요청을 거절한다)
            ("fs", _) => {
                let id: crate::state::TabId = label.parse().unwrap_or_default();
                let on = rest == "on";
                let a = app.clone();
                let _ = app.run_on_main_thread(move || {
                    crate::browser::set_page_fullscreen(&a, id, on);
                    let _ = tx.send(format!("fullscreen={}", crate::browser::is_fullscreen(&a)));
                });
                rx.recv_timeout(Duration::from_secs(5)).unwrap_or_else(|_| "timeout".into())
            }
            // 확장 시험: "ext_add ui <폴더>" → 프로필에 확장 등록, "ext_list ui" → 등록된 확장
            ("ext_add", Some(wv)) | ("ext_list", Some(wv)) => {
                let add = verb == "ext_add";
                let _ = wv.with_webview(move |p| unsafe {
                    let Ok(core) = p.controller().CoreWebView2() else { return };
                    let Ok(profile) = core.cast::<ICoreWebView2_13>().and_then(|c| c.Profile()).and_then(|pr| pr.cast::<ICoreWebView2Profile7>()) else {
                        let _ = tx.send("Profile7 없음".into());
                        return;
                    };
                    use windows::core::{Interface, PWSTR};
                    let name_of = |e: &ICoreWebView2BrowserExtension| {
                        let (mut id, mut name) = (PWSTR::null(), PWSTR::null());
                        let _ = e.Id(&mut id);
                        let _ = e.Name(&mut name);
                        format!("{} {}", id.to_string().unwrap_or_default(), name.to_string().unwrap_or_default())
                    };
                    if add {
                        let tx2 = tx.clone();
                        let h = ProfileAddBrowserExtensionCompletedHandler::create(Box::new(move |err, ext| {
                            let _ = tx2.send(match ext { Some(e) if err.is_ok() => format!("added {}", name_of(&e)), _ => format!("ERR {err:?}") });
                            Ok(())
                        }));
                        if let Err(e) = profile.AddBrowserExtension(&HSTRING::from(rest), &h) {
                            let _ = tx.send(format!("ERR call {e:?}"));
                        }
                    } else {
                        let tx2 = tx.clone();
                        let h = ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(move |err, list| {
                            let mut out = vec![format!("{err:?}")];
                            if let Some(l) = list {
                                let mut n = 0u32;
                                let _ = l.Count(&mut n);
                                for i in 0..n {
                                    if let Ok(e) = l.GetValueAtIndex(i) {
                                        out.push(name_of(&e));
                                    }
                                }
                            }
                            let _ = tx2.send(out.join(" | "));
                            Ok(())
                        }));
                        let _ = profile.GetBrowserExtensions(&h);
                    }
                });
                rx.recv_timeout(Duration::from_secs(20)).unwrap_or_else(|_| "timeout".into())
            }
            // 업데이트 시험: "update" → 확인해 둔 새 버전을 내려받아 서명을 검사한다 (설치하지 않는다)
            ("update", _) => tauri::async_runtime::block_on(crate::updater::dev_download(&app)),
            // DevTools 명령을 그대로 보낸다: "cdp <label> <method> <json>"
            ("cdp", Some(wv)) => {
                let (method, params) = rest.split_once(' ').map(|(m, p)| (m.to_string(), p.to_string())).unwrap_or((rest.clone(), "{}".into()));
                let _ = wv.with_webview(move |p| unsafe {
                    let Ok(core) = p.controller().CoreWebView2() else { return };
                    let h = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |err, res| {
                        let _ = tx.send(if err.is_ok() { res } else { format!("ERR {err:?}") });
                        Ok(())
                    }));
                    let _ = core.CallDevToolsProtocolMethod(&HSTRING::from(method), &HSTRING::from(params), &h);
                });
                rx.recv_timeout(Duration::from_secs(15)).unwrap_or_else(|_| "timeout".into())
            }
            ("capture", Some(wv)) => {
                let path = rest;
                let _ = wv.with_webview(move |p| unsafe {
                    use windows::Win32::{System::Com::*, UI::Shell::SHCreateMemStream};
                    let Ok(core) = p.controller().CoreWebView2() else { return };
                    let Some(stream) = SHCreateMemStream(None) else { return };
                    let s2 = stream.clone();
                    let h = CapturePreviewCompletedHandler::create(Box::new(move |err| {
                        let mut out = Vec::new();
                        if err.is_ok() && s2.Seek(0, STREAM_SEEK_SET, None).is_ok() {
                            let mut buf = [0u8; 65536];
                            loop {
                                let mut n = 0u32;
                                if s2.Read(buf.as_mut_ptr() as _, buf.len() as u32, Some(&mut n)).is_err() || n == 0 {
                                    break;
                                }
                                out.extend_from_slice(&buf[..n as usize]);
                            }
                        }
                        let _ = tx.send(match std::fs::write(&path, &out) {
                            Ok(()) if !out.is_empty() => format!("ok {} bytes", out.len()),
                            r => format!("ERR {err:?} {r:?}"),
                        });
                        Ok(())
                    }));
                    let _ = core.CapturePreview(COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG, &stream, &h);
                });
                rx.recv_timeout(Duration::from_secs(15)).unwrap_or_else(|_| "timeout".into())
            }
            (_, None) if verb == "js" || verb == "capture" => format!("no webview {label}"),
            _ => format!("unknown command: {cmd}"),
        };
        let _ = std::fs::write(&done_file, result);
    }
}
