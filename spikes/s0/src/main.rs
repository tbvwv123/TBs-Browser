//! TB's Browser 0단계 기술 검증 (docs/spec.md 9장, 10장).
//! 본 기능 코드가 아니다. 10장의 각 항목을 실제로 돌려 보고 결과를 out/report.jsonl에 남긴다.
//!
//! 실행: `cargo run` — 사용자 시험 모드. `S0_AUTORUN=1 cargo run` — 자동 시험(테스트 서버 testsite/server.py 필요)
//! 외부 시험 스크립트는 out/cmd.txt에 명령 한 줄을 쓰고, 결과는 out/cmd-done.txt로 받는다. (시험 전용 통로)
#![allow(clippy::missing_safety_doc)]

use std::{
    fs::{self, OpenOptions},
    io::Write as _,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{channel, Sender},
        Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use tauri::{
    webview::{PlatformWebview, WebviewBuilder},
    window::WindowBuilder,
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Webview, WebviewUrl, WebviewWindowBuilder,
};
use webview2_com::{Microsoft::Web::WebView2::Win32::*, *};
use windows::{
    core::{w, Interface, BOOL, HSTRING, PCWSTR, PWSTR},
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::{LibraryLoader::GetModuleHandleW, ProcessStatus::*, Threading::*},
        UI::{Input::KeyboardAndMouse::GetKeyState, Shell::SHCreateMemStream, WindowsAndMessaging::*},
    },
};

const APP_NAME: &str = "TB's Browser";
const SITE: &str = "http://127.0.0.1:8765";

static OUT: OnceLock<PathBuf> = OnceLock::new();
static APP: OnceLock<AppHandle> = OnceLock::new();
static CARD: Mutex<(f64, f64, f64, f64)> = Mutex::new((0.0, 0.0, 800.0, 600.0));
static ROUND: Mutex<f64> = Mutex::new(12.0);
static OVERRIDE_HITS: AtomicUsize = AtomicUsize::new(0);
static OVERRIDE_URIS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static AUTORUN_DONE: AtomicBool = AtomicBool::new(false);

fn out_dir() -> &'static Path {
    OUT.get().unwrap()
}

/// 결과 한 줄을 report.jsonl에 추가한다.
fn report(item: &str, name: &str, value: Value) {
    let line = json!({ "item": item, "test": name, "result": value });
    eprintln!("[S0] {line}");
    let mut f = OpenOptions::new().create(true).append(true).open(out_dir().join("report.jsonl")).unwrap();
    writeln!(f, "{line}").unwrap();
    if let Some(app) = APP.get() {
        let _ = app.emit_to(tauri::EventTarget::webview("ui"), "s0-log", format!("[{item}] {name}"));
    }
}

fn event_log(msg: &str) {
    let mut f = OpenOptions::new().create(true).append(true).open(out_dir().join("events.log")).unwrap();
    writeln!(f, "{} {msg}", chrono_ms()).unwrap();
    if let Some(app) = APP.get() {
        if !msg.starts_with("window ") && !msg.starts_with("snap-overlay hover") {
            let _ = app.emit_to(tauri::EventTarget::webview("ui"), "s0-log", msg.to_string());
        }
    }
}

fn chrono_ms() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()
}

// ───────────────────────── 명령 (UI 웹뷰에만 허용) ─────────────────────────

#[tauri::command]
fn s0_ping(webview: Webview) -> String {
    let msg = format!("pong from host (called by webview '{}')", webview.label());
    event_log(&format!("s0_ping called by {}", webview.label()));
    msg
}

#[tauri::command]
fn ui_log(msg: String) {
    event_log(&msg);
}

#[tauri::command]
fn set_content_bounds(app: AppHandle, x: f64, y: f64, w: f64, h: f64) {
    *CARD.lock().unwrap() = (x, y, w, h);
    apply_bounds(&app);
}

#[tauri::command]
fn show_tab(app: AppHandle, label: String) {
    for l in ["c1", "c2"] {
        if let Some(wv) = app.get_webview(l) {
            if l == label {
                let _ = wv.show();
            } else {
                let _ = wv.hide();
            }
        }
    }
    event_log(&format!("show_tab {label}"));
}

fn content_labels() -> [&'static str; 3] {
    ["c1", "c2", "c3"]
}

fn apply_bounds(app: &AppHandle) {
    let (x, y, w, h) = *CARD.lock().unwrap();
    for l in content_labels() {
        if let Some(wv) = app.get_webview(l) {
            let _ = wv.set_position(LogicalPosition::new(x, y));
            let _ = wv.set_size(LogicalSize::new(w, h));
            apply_round(&wv);
        }
    }
}

/// 10장 4번: 웹뷰를 담은 자식 창(WRY_WEBVIEW)에 둥근 사각형 영역을 건다.
fn apply_round(wv: &Webview) {
    let r = *ROUND.lock().unwrap();
    let scale = wv.window().scale_factor().unwrap_or(1.0);
    let (_, _, w, h) = *CARD.lock().unwrap();
    let (pw, ph, pr) = ((w * scale).round() as i32, (h * scale).round() as i32, (r * scale).round() as i32);
    let _ = wv.with_webview(move |p: PlatformWebview| unsafe {
        let mut hwnd = HWND::default();
        if p.controller().ParentWindow(&mut hwnd).is_ok() {
            if pr <= 0 {
                SetWindowRgn(hwnd, None, true);
            } else {
                let rgn = CreateRoundRectRgn(0, 0, pw + 1, ph + 1, pr * 2, pr * 2);
                SetWindowRgn(hwnd, Some(rgn), true);
            }
        }
    });
}

// ───────────────────────── 네이티브 호출 도우미 ─────────────────────────

/// 메인 스레드에서 PlatformWebview로 작업하고, 비동기 결과를 채널로 기다린다.
fn on_main<T: Send + 'static>(
    wv: &Webview,
    timeout: Duration,
    f: impl FnOnce(PlatformWebview, Sender<T>) + Send + 'static,
) -> Option<T> {
    let (tx, rx) = channel();
    wv.with_webview(move |p| f(p, tx)).ok()?;
    rx.recv_timeout(timeout).ok()
}

fn core(p: &PlatformWebview) -> ICoreWebView2 {
    unsafe { p.controller().CoreWebView2().unwrap() }
}

/// ExecuteScript(네이티브)로 스크립트를 돌린다. 결과는 JSON 값.
fn js(wv: &Webview, script: &str) -> Value {
    let script = script.to_string();
    let r = on_main(wv, Duration::from_secs(15), move |p, tx| unsafe {
        let h = ExecuteScriptCompletedHandler::create(Box::new(move |err, res| {
            let _ = tx.send(match err {
                Ok(()) => res,
                Err(e) => json!({ "execute_error": format!("{e:?}") }).to_string(),
            });
            Ok(())
        }));
        let _ = core(&p).ExecuteScript(&HSTRING::from(script), &h);
    });
    match r {
        Some(s) => serde_json::from_str(&s).unwrap_or(Value::String(s)),
        None => json!({ "error": "timeout" }),
    }
}

/// 비동기 JS 본문을 돌리고 결과를 기다린다(Promise는 ExecuteScript가 기다려 주지 않는다).
fn js_async(wv: &Webview, body: &str, timeout: Duration) -> Value {
    let id = format!("__s0r{}", chrono_ms());
    js(
        wv,
        &format!(
            "window.{id}=undefined;(async()=>{{{body}}})().then(v=>window.{id}={{ok:v}},e=>window.{id}={{err:String(e)}});1"
        ),
    );
    let start = Instant::now();
    while start.elapsed() < timeout {
        thread::sleep(Duration::from_millis(150));
        let v = js(wv, &format!("window.{id}===undefined?null:window.{id}"));
        if !v.is_null() {
            return v;
        }
    }
    json!({ "error": "timeout" })
}

fn wait_loaded(wv: &Webview, url_part: &str) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(30) {
        let v = js(wv, "[document.readyState, location.href]");
        if v[0] == "complete" && v[1].as_str().map(|s| s.contains(url_part)).unwrap_or(false) {
            return true;
        }
        thread::sleep(Duration::from_millis(200));
    }
    false
}

fn nav(wv: &Webview, url: &str) -> bool {
    let _ = wv.navigate(url.parse().unwrap());
    thread::sleep(Duration::from_millis(300));
    let part = url.split('#').next().unwrap().trim_end_matches('/');
    wait_loaded(wv, part)
}

fn browser_pid(wv: &Webview) -> Option<u32> {
    on_main(wv, Duration::from_secs(5), |p, tx| unsafe {
        let mut pid = 0u32;
        let _ = core(&p).BrowserProcessId(&mut pid);
        let _ = tx.send(pid);
    })
}

/// 이 브라우저에 속한 WebView2 프로세스 목록과 메모리 (ICoreWebView2Environment8::GetProcessInfos).
fn processes(wv: &Webview) -> Vec<(u32, String)> {
    on_main(wv, Duration::from_secs(5), |p, tx| unsafe {
        let mut v = Vec::new();
        if let Ok(env8) = p.environment().cast::<ICoreWebView2Environment8>() {
            if let Ok(col) = env8.GetProcessInfos() {
                let mut n = 0u32;
                let _ = col.Count(&mut n);
                for i in 0..n {
                    if let Ok(info) = col.GetValueAtIndex(i) {
                        let (mut pid, mut kind) = (0i32, COREWEBVIEW2_PROCESS_KIND::default());
                        let _ = info.ProcessId(&mut pid);
                        let _ = info.Kind(&mut kind);
                        let k = match kind {
                            COREWEBVIEW2_PROCESS_KIND_BROWSER => "browser",
                            COREWEBVIEW2_PROCESS_KIND_RENDERER => "renderer",
                            COREWEBVIEW2_PROCESS_KIND_GPU => "gpu",
                            COREWEBVIEW2_PROCESS_KIND_UTILITY => "utility",
                            _ => "other",
                        };
                        v.push((pid as u32, k.to_string()));
                    }
                }
            }
        }
        let _ = tx.send(v);
    })
    .unwrap_or_default()
}

/// (private bytes, working set) — 바이트
fn mem_of(pid: u32) -> (u64, u64) {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return (0, 0) };
        let mut c = PROCESS_MEMORY_COUNTERS_EX::default();
        let ok = K32GetProcessMemoryInfo(h, &mut c as *mut _ as *mut _, std::mem::size_of_val(&c) as u32).as_bool();
        let _ = CloseHandle(h);
        if ok { (c.PrivateUsage as u64, c.WorkingSetSize as u64) } else { (0, 0) }
    }
}

fn mem_snapshot(wv: &Webview) -> Value {
    let procs = processes(wv);
    let host = mem_of(std::process::id());
    let mut total_priv = host.0;
    let mut total_ws = host.1;
    let mut list = vec![json!({"pid": std::process::id(), "kind": "host(tauri)", "private_mb": mb(host.0), "ws_mb": mb(host.1)})];
    for (pid, kind) in procs {
        let (pv, ws) = mem_of(pid);
        total_priv += pv;
        total_ws += ws;
        list.push(json!({"pid": pid, "kind": kind, "private_mb": mb(pv), "ws_mb": mb(ws)}));
    }
    json!({ "total_private_mb": mb(total_priv), "total_ws_mb": mb(total_ws), "processes": list })
}

fn mb(b: u64) -> f64 {
    (b as f64 / 1048576.0 * 10.0).round() / 10.0
}

fn try_suspend(wv: &Webview) -> Value {
    let r = on_main(wv, Duration::from_secs(15), |p, tx| unsafe {
        let Ok(c3) = core(&p).cast::<ICoreWebView2_3>() else {
            let _ = tx.send("ICoreWebView2_3 cast 실패".to_string());
            return;
        };
        let tx2 = tx.clone();
        let h = TrySuspendCompletedHandler::create(Box::new(move |err, ok| {
            let _ = tx2.send(format!("hr={err:?} is_successful={ok}"));
            Ok(())
        }));
        if let Err(e) = c3.TrySuspend(&h) {
            let _ = tx.send(format!("TrySuspend 호출 실패: {e:?}"));
        }
    });
    json!(r.unwrap_or_else(|| "timeout".into()))
}

fn is_suspended(wv: &Webview) -> Value {
    json!(on_main(wv, Duration::from_secs(5), |p, tx| unsafe {
        let mut b = BOOL(0);
        let r = core(&p).cast::<ICoreWebView2_3>().and_then(|c| c.IsSuspended(&mut b));
        let _ = tx.send(format!("{:?} suspended={}", r.map(|_| ()), b.as_bool()));
    }))
}

fn resume(wv: &Webview) -> Value {
    json!(on_main(wv, Duration::from_secs(5), |p, tx| unsafe {
        let r = core(&p).cast::<ICoreWebView2_3>().and_then(|c| c.Resume());
        let _ = tx.send(format!("{r:?}"));
    }))
}

fn set_mem_level(wv: &Webview, low: bool) -> Value {
    json!(on_main(wv, Duration::from_secs(5), move |p, tx| unsafe {
        let lvl = if low { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW } else { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL };
        let r = core(&p).cast::<ICoreWebView2_19>().and_then(|c| c.SetMemoryUsageTargetLevel(lvl));
        let _ = tx.send(format!("{r:?}"));
    }))
}

fn capture_preview(wv: &Webview, file: PathBuf) -> Value {
    let r = on_main(wv, Duration::from_secs(15), move |p, tx| unsafe {
        let Some(stream) = SHCreateMemStream(None) else {
            let _ = tx.send(Err("SHCreateMemStream 실패".to_string()));
            return;
        };
        let s2 = stream.clone();
        let tx2 = tx.clone();
        let h = CapturePreviewCompletedHandler::create(Box::new(move |err| {
            let res = (|| -> std::result::Result<usize, String> {
                err.map_err(|e| format!("{e:?}"))?;
                use windows::Win32::System::Com::*;
                let mut size = 0u64;
                s2.Seek(0, STREAM_SEEK_END, Some(&mut size)).map_err(|e| format!("{e:?}"))?;
                s2.Seek(0, STREAM_SEEK_SET, None).map_err(|e| format!("{e:?}"))?;
                let mut buf = vec![0u8; size as usize];
                let mut read = 0u32;
                s2.Read(buf.as_mut_ptr() as _, size as u32, Some(&mut read)).ok().map_err(|e| format!("{e:?}"))?;
                fs::write(&file, &buf).map_err(|e| e.to_string())?;
                Ok(buf.len())
            })();
            let _ = tx2.send(res);
            Ok(())
        }));
        if let Err(e) = core(&p).CapturePreview(COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG, &stream, &h) {
            let _ = tx.send(Err(format!("CapturePreview 호출 실패: {e:?}")));
        }
    });
    match r {
        Some(Ok(n)) => json!({ "ok": true, "bytes": n }),
        Some(Err(e)) => json!({ "ok": false, "error": e }),
        None => json!({ "ok": false, "error": "timeout" }),
    }
}

fn add_doc_created_script(wv: &Webview, script: &str) -> Value {
    let s = script.to_string();
    json!(on_main(wv, Duration::from_secs(10), move |p, tx| unsafe {
        let tx2 = tx.clone();
        let h = AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(move |err, id| {
            let _ = tx2.send(format!("hr={err:?} id={id}"));
            Ok(())
        }));
        if let Err(e) = core(&p).AddScriptToExecuteOnDocumentCreated(&HSTRING::from(s), &h) {
            let _ = tx.send(format!("호출 실패 {e:?}"));
        }
    }))
}

/// 10장 9번: 리소스 바꿔치기 처리기. 걸린 요청에 파란 PNG를 돌려준다.
fn install_override_handler(wv: &Webview) -> Value {
    json!(on_main(wv, Duration::from_secs(5), |p, tx| unsafe {
        let env = p.environment();
        let blue: &'static [u8] = include_bytes!("../icons/blue.png");
        let h = WebResourceRequestedEventHandler::create(Box::new(move |_wv, args| {
            let Some(args) = args else { return Ok(()) };
            let req = args.Request()?;
            let mut uri = PWSTR::null();
            req.Uri(&mut uri)?;
            let uri = take_pwstr(uri);
            let mut kind = COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS::default();
            if let Ok(a2) = args.cast::<ICoreWebView2WebResourceRequestedEventArgs2>() {
                let _ = a2.RequestedSourceKind(&mut kind);
            }
            OVERRIDE_HITS.fetch_add(1, Ordering::SeqCst);
            OVERRIDE_URIS.lock().unwrap().push(format!("{uri} source_kind={}", kind.0));
            let stream = SHCreateMemStream(Some(blue));
            let resp = env.CreateWebResourceResponse(
                stream.as_ref(),
                200,
                &HSTRING::from("OK"),
                &HSTRING::from("Content-Type: image/png\r\nCache-Control: no-store"),
            )?;
            args.SetResponse(&resp)?;
            Ok(())
        }));
        let mut token = 0i64;
        let r = core(&p).add_WebResourceRequested(&h, &mut token);
        let _ = tx.send(format!("{r:?}"));
    }))
}

fn add_filter(wv: &Webview, pattern: &str, all_sources: bool) -> Value {
    let pat = pattern.to_string();
    json!(on_main(wv, Duration::from_secs(5), move |p, tx| unsafe {
        let c = core(&p);
        let r = if all_sources {
            c.cast::<ICoreWebView2_22>().and_then(|c22| {
                c22.AddWebResourceRequestedFilterWithRequestSourceKinds(
                    &HSTRING::from(pat),
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                    COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
                )
            })
        } else {
            c.AddWebResourceRequestedFilter(&HSTRING::from(pat), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)
        };
        let _ = tx.send(format!("{r:?}"));
    }))
}

fn remove_filters(wv: &Webview, pattern: &str) {
    let pat = pattern.to_string();
    on_main(wv, Duration::from_secs(5), move |p, tx| unsafe {
        let c = core(&p);
        let _ = c.RemoveWebResourceRequestedFilter(&HSTRING::from(pat.clone()), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL);
        if let Ok(c22) = c.cast::<ICoreWebView2_22>() {
            let _ = c22.RemoveWebResourceRequestedFilterWithRequestSourceKinds(
                &HSTRING::from(pat),
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
            );
        }
        let _ = tx.send(());
    });
}

/// 10장 7번: 콘텐츠 웹뷰에 포커스가 있을 때 단축키를 호스트가 받는지.
fn install_accelerator_handler(wv: &Webview) -> Value {
    let label = wv.label().to_string();
    json!(on_main(wv, Duration::from_secs(5), move |p, tx| unsafe {
        let h = AcceleratorKeyPressedEventHandler::create(Box::new(move |_c, args| {
            let Some(args) = args else { return Ok(()) };
            let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
            let mut vk = 0u32;
            args.KeyEventKind(&mut kind)?;
            args.VirtualKey(&mut vk)?;
            let down = kind == COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN || kind == COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN;
            if !down {
                return Ok(());
            }
            let ctrl = GetKeyState(0x11) < 0;
            let shift = GetKeyState(0x10) < 0;
            let alt = GetKeyState(0x12) < 0;
            let name = vk_name(vk);
            let combo = format!("{}{}{}{}", if ctrl { "Ctrl+" } else { "" }, if shift { "Shift+" } else { "" }, if alt { "Alt+" } else { "" }, name);
            // 브라우저 단축키로 쓸 조합은 페이지로 넘기지 않는다 (Ctrl+F, F12는 WebView2 기본 동작에 맡긴다)
            let ours = matches!(
                combo.as_str(),
                "Ctrl+T" | "Ctrl+W" | "Ctrl+Shift+T" | "Ctrl+Tab" | "Ctrl+Shift+Tab" | "Ctrl+L" | "Ctrl+R" | "F5"
                    | "Alt+Left" | "Alt+Right" | "Ctrl+D" | "Ctrl+H" | "Ctrl+J" | "Ctrl+B" | "Ctrl+Shift+A" | "F11"
            );
            if ours {
                args.SetHandled(true)?;
            }
            event_log(&format!("accelerator webview={label} combo={combo} handled={ours}"));
            Ok(())
        }));
        let mut token = 0i64;
        let r = p.controller().add_AcceleratorKeyPressed(&h, &mut token);
        let _ = tx.send(format!("{r:?}"));
    }))
}

fn vk_name(vk: u32) -> String {
    match vk {
        0x09 => "Tab".into(),
        0x25 => "Left".into(),
        0x27 => "Right".into(),
        0x70..=0x7B => format!("F{}", vk - 0x6F),
        0x41..=0x5A | 0x30..=0x39 => char::from_u32(vk).unwrap().to_string(),
        0x10 | 0x11 | 0x12 => "(mod)".into(),
        _ => format!("vk{vk:#x}"),
    }
}

fn set_password_settings(wv: &Webview) -> Value {
    json!(on_main(wv, Duration::from_secs(5), |p, tx| unsafe {
        let r = (|| -> windows::core::Result<String> {
            let s4 = core(&p).Settings()?.cast::<ICoreWebView2Settings4>()?;
            s4.SetIsPasswordAutosaveEnabled(true)?;
            s4.SetIsGeneralAutofillEnabled(true)?;
            let (mut a, mut b) = (BOOL(0), BOOL(0));
            s4.IsPasswordAutosaveEnabled(&mut a)?;
            s4.IsGeneralAutofillEnabled(&mut b)?;
            Ok(format!("IsPasswordAutosaveEnabled={} IsGeneralAutofillEnabled={}", a.as_bool(), b.as_bool()))
        })();
        let _ = tx.send(format!("{r:?}"));
    }))
}

fn focus_content(wv: &Webview) -> Value {
    json!(on_main(wv, Duration::from_secs(5), |p, tx| unsafe {
        let r = p.controller().MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
        let _ = tx.send(format!("{r:?}"));
    }))
}

fn user_data_folder(wv: &Webview) -> Value {
    json!(on_main(wv, Duration::from_secs(5), |p, tx| unsafe {
        let r = p.environment().cast::<ICoreWebView2Environment7>().and_then(|e| {
            let mut s = PWSTR::null();
            e.UserDataFolder(&mut s)?;
            Ok(take_pwstr(s))
        });
        let _ = tx.send(format!("{r:?}"));
    }))
}

fn env_ptr(wv: &Webview) -> Value {
    json!(on_main(wv, Duration::from_secs(5), |p, tx| {
        let _ = tx.send(format!("{:p}", p.environment().as_raw()));
    }))
}

fn add_extension(wv: &Webview, dir: PathBuf) -> Value {
    json!(on_main(wv, Duration::from_secs(15), move |p, tx| unsafe {
        let r = core(&p)
            .cast::<ICoreWebView2_13>()
            .and_then(|c| c.Profile())
            .and_then(|pr| pr.cast::<ICoreWebView2Profile7>());
        match r {
            Err(e) => {
                let _ = tx.send(format!("Profile7 cast 실패 {e:?}"));
            }
            Ok(profile) => {
                let tx2 = tx.clone();
                let h = ProfileAddBrowserExtensionCompletedHandler::create(Box::new(move |err, ext| {
                    let id = ext
                        .map(|e| {
                            let mut s = PWSTR::null();
                            let _ = e.Id(&mut s);
                            take_pwstr(s)
                        })
                        .unwrap_or_default();
                    let _ = tx2.send(format!("hr={err:?} extension_id={id}"));
                    Ok(())
                }));
                if let Err(e) = profile.AddBrowserExtension(&HSTRING::from(dir.as_path()), &h) {
                    let _ = tx.send(format!("AddBrowserExtension 호출 실패 {e:?}"));
                }
            }
        }
    }))
}


/// 디스크 캐시만 비운다 (ICoreWebView2Profile2::ClearBrowsingData)
fn clear_cache(wv: &Webview) -> Value {
    json!(on_main(wv, Duration::from_secs(15), |p, tx| unsafe {
        let r = core(&p).cast::<ICoreWebView2_13>().and_then(|c| c.Profile()).and_then(|pr| pr.cast::<ICoreWebView2Profile2>());
        match r {
            Err(e) => { let _ = tx.send(format!("cast 실패 {e:?}")); }
            Ok(pr) => {
                let tx2 = tx.clone();
                let h = ClearBrowsingDataCompletedHandler::create(Box::new(move |err| { let _ = tx2.send(format!("{err:?}")); Ok(()) }));
                if let Err(e) = pr.ClearBrowsingData(COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE, &h) { let _ = tx.send(format!("호출 실패 {e:?}")); }
            }
        }
    }))
}

// ───────────────────────── 10장 5번: 팝업 ─────────────────────────

fn ensure_popup_child(app: &AppHandle) -> Webview {
    if let Some(w) = app.get_webview("popup") {
        return w;
    }
    let win = app.get_window("main").unwrap();
    let b = WebviewBuilder::new("popup", WebviewUrl::App("popup.html#flat".into()))
        .data_directory(out_dir().join("webview-data"))
        .browser_extensions_enabled(true);
    win.add_child(b, LogicalPosition::new(0.0, 0.0), LogicalSize::new(10.0, 10.0)).unwrap()
}

// ───────────────────────── 10장 6번: 스냅 레이아웃용 덮개 창 ─────────────────────────

static SNAP_HWND: Mutex<isize> = Mutex::new(0);

unsafe extern "system" fn snap_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_NCHITTEST => LRESULT(HTMAXBUTTON as isize),
            WM_NCLBUTTONDOWN => LRESULT(0),
            WM_NCLBUTTONUP => {
                if let Ok(parent) = GetParent(hwnd) {
                    let mut wp2 = WINDOWPLACEMENT { length: std::mem::size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
                    let _ = GetWindowPlacement(parent, &mut wp2);
                    let cmd = if wp2.showCmd == SW_SHOWMAXIMIZED.0 as u32 { SW_RESTORE } else { SW_MAXIMIZE };
                    let _ = ShowWindow(parent, cmd);
                    event_log(&format!("snap-overlay click -> {}", if cmd == SW_RESTORE { "restore" } else { "maximize" }));
                }
                LRESULT(0)
            }
            WM_NCMOUSEMOVE => {
                event_log("snap-overlay hover");
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

/// 최대화 버튼 자리에 거의 투명한 자식 창을 올려 WM_NCHITTEST에 HTMAXBUTTON을 돌려준다.
fn install_snap_overlay(app: &AppHandle) {
    let win = app.get_window("main").unwrap();
    let parent_raw = win.hwnd().unwrap().0 as isize;
    let _ = win.run_on_main_thread(move || unsafe {
        let parent = HWND(parent_raw as _);
        let class = w!("S0_SNAP_OVERLAY");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(snap_proc),
            hInstance: HINSTANCE(GetModuleHandleW(PCWSTR::null()).unwrap_or_default().0),
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&wc);
        let layered = std::env::var("S0_SNAP_LAYERED").is_ok(); // 레이어드 자식 창은 생성 실패(결과 문서 6번)
        let ex = if layered { WS_EX_LAYERED | WS_EX_NOACTIVATE } else { WS_EX_NOACTIVATE };
        let created = CreateWindowExW(
            ex,
            class,
            PCWSTR::null(),
            WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
            0,
            0,
            40,
            30,
            Some(parent),
            None,
            Some(wc.hInstance),
            None,
        );
        event_log(&format!("snap-overlay create layered={layered} -> {:?}", created.as_ref().map(|h| h.0 as isize)));
        if let Ok(h) = created {
            if layered {
                let r = SetLayeredWindowAttributes(h, COLORREF(0), 1, LWA_ALPHA);
                event_log(&format!("snap-overlay SetLayeredWindowAttributes -> {r:?}"));
            }
            let _ = SetWindowPos(h, Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            *SNAP_HWND.lock().unwrap() = h.0 as isize;
        }
    });
}

/// 덮개 창을 최대화 버튼(오른쪽 위 두 번째 40×30) 자리에 맞춘다.
fn place_snap_overlay(app: &AppHandle) {
    let win = app.get_window("main").unwrap();
    let scale = win.scale_factor().unwrap_or(1.0);
    let size = win.inner_size().unwrap();
    let h = *SNAP_HWND.lock().unwrap();
    if h == 0 {
        return;
    }
    let (bw, bh) = ((40.0 * scale) as i32, (30.0 * scale) as i32);
    // 패딩 8 + 창 버튼 묶음 맨 오른쪽이 닫기, 그 왼쪽이 최대화. 막대 높이 34, 버튼 30 → 위 2px 여백
    let x = size.width as i32 - (8.0 * scale) as i32 - bw * 2;
    let y = (8.0 * scale) as i32 + (2.0 * scale) as i32;
    let _ = win.run_on_main_thread(move || unsafe {
        let r = SetWindowPos(HWND(h as _), Some(HWND_TOP), x, y, bw, bh, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        event_log(&format!("snap-overlay placed at {x},{y} {bw}x{bh} -> {r:?}"));
    });
}


// ───────────────────────── 개발용: 가상 데스크톱 2에서 띄우기 ─────────────────────────

/// TBS_DEV_DESKTOP(가상 데스크톱 GUID)이 있으면 창을 그 데스크톱으로 옮긴다. 사용자의 화면을 건드리지 않기 위한 개발용.
fn move_to_dev_desktop(hwnd_raw: isize) -> Option<String> {
    let g = std::env::var("TBS_DEV_DESKTOP").ok()?;
    unsafe {
        use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
        use windows::Win32::UI::Shell::{IVirtualDesktopManager, VirtualDesktopManager};
        let r = (|| -> windows::core::Result<()> {
            let guid = windows::core::GUID::try_from(g.as_str()).map_err(|_| windows::core::Error::from(E_INVALIDARG))?;
            let m: IVirtualDesktopManager = CoCreateInstance(&VirtualDesktopManager, None, CLSCTX_ALL)?;
            m.MoveWindowToDesktop(HWND(hwnd_raw as _), &guid)
        })();
        Some(format!("{r:?}"))
    }
}

// ───────────────────────── 자동 시험 ─────────────────────────

fn autorun(app: AppHandle) {
    thread::sleep(Duration::from_secs(2));
    let c1 = app.get_webview("c1").unwrap();
    let c2 = app.get_webview("c2").unwrap();
    let ui = app.get_webview("ui").unwrap();

    // ── 1번: 자식 웹뷰 배치·보이기·숨기기
    report("1", "webviews in main window", json!(app.get_window("main").unwrap().webviews().iter().map(|w| w.label().to_string()).collect::<Vec<_>>()));
    let ok1 = nav(&c1, "https://example.com/");
    let ok2 = nav(&c2, "https://www.wikipedia.org/");
    report("1", "c1/c2 loaded", json!({"c1 example.com": ok1, "c2 wikipedia.org": ok2}));
    thread::sleep(Duration::from_secs(1));
    report("1", "bounds after set_content_bounds", json!({
        "card(logical)": format!("{:?}", *CARD.lock().unwrap()),
        "c1": format!("{:?} {:?}", c1.position().ok(), c1.size().ok()),
        "c2": format!("{:?} {:?}", c2.position().ok(), c2.size().ok()),
    }));
    // 위치·크기 바꾸기 시험 후 원복
    let _ = c2.set_position(LogicalPosition::new(300.0, 120.0));
    let _ = c2.set_size(LogicalSize::new(400.0, 300.0));
    thread::sleep(Duration::from_millis(500));
    report("1", "c2 moved+resized to (300,120) 400x300 logical", json!(format!("{:?} {:?}", c2.position().ok(), c2.size().ok())));
    apply_bounds(&app);
    for i in 0..4 {
        show_tab(app.clone(), if i % 2 == 0 { "c2".into() } else { "c1".into() });
        thread::sleep(Duration::from_millis(400));
    }
    report("1", "show/hide toggled 4 times", json!({
        "c1 visibility": js(&c1, "document.visibilityState"),
        "c2 visibility": js(&c2, "document.visibilityState"),
    }));

    // ── 2번: 네이티브 WebView2 객체
    report("2", "AddScriptToExecuteOnDocumentCreated", add_doc_created_script(&c1, "window.__s0injected = 'injected-before-page:' + location.href;"));
    nav(&c1, "https://example.com/?inj");
    report("2", "injected script ran on next document", js(&c1, "window.__s0injected || null"));
    report("2", "CapturePreview(PNG) c1", capture_preview(&c1, out_dir().join("capture-c1.png")));
    report("2", "TrySuspend/Resume interface present", json!({"is_suspended": is_suspended(&c2)}));
    report("2", "WebResourceRequested handler install", install_override_handler(&c1));

    // ── 3번: 환경·사용자 데이터 폴더 공유
    report("3", "BrowserProcessId", json!({"ui": browser_pid(&ui), "c1": browser_pid(&c1), "c2": browser_pid(&c2)}));
    report("3", "UserDataFolder", json!({"ui": user_data_folder(&ui), "c1": user_data_folder(&c1), "c2": user_data_folder(&c2)}));
    report("3", "ICoreWebView2Environment object pointer", json!({"ui": env_ptr(&ui), "c1": env_ptr(&c1), "c2": env_ptr(&c2)}));
    nav(&c1, &format!("{SITE}/"));
    js(&c1, "document.cookie='s0login=yes-from-c1; max-age=3600; path=/'; document.cookie");
    nav(&c2, &format!("{SITE}/"));
    report("3", "cookie set in c1, read in c2", js(&c2, "document.cookie"));
    js(&c1, "localStorage.setItem('s0ls','from-c1')");
    report("3", "localStorage set in c1, read in c2", js(&c2, "localStorage.getItem('s0ls')"));

    // ── 10번: 콘텐츠 웹뷰에서 앱 명령
    let probe = r#"
      const out = {};
      out.has_tauri_internals = typeof window.__TAURI_INTERNALS__;
      out.has_tauri_global = typeof window.__TAURI__;
      out.has_chrome_webview = typeof (window.chrome && window.chrome.webview);
      const tryInvoke = async (cmd, args) => {
        try { return 'OK ' + JSON.stringify(await window.__TAURI_INTERNALS__.invoke(cmd, args || {})); }
        catch (e) { return 'ERR ' + String(e); }
      };
      if (window.__TAURI_INTERNALS__) {
        out.invoke_s0_ping = await tryInvoke('s0_ping');
        out.invoke_show_tab = await tryInvoke('show_tab', { label: 'c2' });
        out.invoke_app_version = await tryInvoke('plugin:app|version');
        out.invoke_window_is_maximized = await tryInvoke('plugin:window|is_maximized', { label: 'main' });
      }
      try {
        const r = await fetch('http://ipc.localhost/s0_ping', { method: 'POST', body: '{}', headers: { 'Content-Type': 'application/json', 'Tauri-Callback': '1', 'Tauri-Error': '2' } });
        out.raw_ipc_fetch = r.status + ' ' + (await r.text()).slice(0, 200);
      } catch (e) { out.raw_ipc_fetch = 'ERR ' + String(e); }
      try { window.chrome.webview.postMessage('{"cmd":"s0_ping","callback":1,"error":2,"payload":{}}'); out.raw_postMessage = 'sent'; } catch (e) { out.raw_postMessage = 'ERR ' + e; }
      return out;
    "#;
    nav(&c1, "https://example.com/?ipc");
    report("10", "remote page (https://example.com) → app commands", js_async(&c1, probe, Duration::from_secs(10)));
    nav(&c1, &format!("{SITE}/?ipc"));
    report("10", "localhost page (http://127.0.0.1:8765) → app commands", js_async(&c1, probe, Duration::from_secs(10)));
    let nav_app = nav(&c1, "http://tauri.localhost/index.html");
    report("10", "content webview navigated to app origin http://tauri.localhost", json!({ "loaded": nav_app, "result": js_async(&c1, probe, Duration::from_secs(10)) }));
    report("10", "UI webview → s0_ping (positive control)", js_async(&ui, "return await window.__TAURI_INTERNALS__.invoke('s0_ping')", Duration::from_secs(5)));
    let ev = fs::read_to_string(out_dir().join("events.log")).unwrap_or_default();
    report("10", "s0_ping callers seen by host", json!(ev.lines().filter(|l| l.contains("s0_ping called")).collect::<Vec<_>>()));
    show_tab(app.clone(), "c1".into());

    // ── 9번: 리소스 바꿔치기 + 캐시 + 서비스 워커
    let pat = format!("{SITE}/img-*");
    nav(&c1, &format!("{SITE}/cache.html"));
    report("9", "cache.html first load (no filter)", js_async(&c1, "return await waitImg(document.getElementById('a'))", Duration::from_secs(5)));
    report("9", "add plain filter", add_filter(&c1, &pat, false));
    OVERRIDE_HITS.store(0, Ordering::SeqCst);
    nav(&c1, &format!("{SITE}/cache.html?2"));
    report("9", "cache.html again after filter (image has max-age=3600, cached)", json!({
        "color": js_async(&c1, "return await waitImg(document.getElementById('a'))", Duration::from_secs(5)),
        "handler_hits": OVERRIDE_HITS.load(Ordering::SeqCst),
    }));
    let _ = c1.reload();
    thread::sleep(Duration::from_millis(500));
    wait_loaded(&c1, "cache.html");
    report("9", "cache.html after reload()", js_async(&c1, "return await waitImg(document.getElementById('a'))", Duration::from_secs(5)));
    remove_filters(&c1, &pat);
    // 서비스 워커: 등록 → 제어될 때까지 다시 열기
    nav(&c1, &format!("{SITE}/sw.html"));
    js_async(&c1, "for(let i=0;i<50&&!window.__swready;i++) await new Promise(r=>setTimeout(r,100)); return !!window.__swready", Duration::from_secs(8));
    nav(&c1, &format!("{SITE}/sw.html?controlled"));
    report("9", "sw.html controlled, no filter", js_async(&c1, "return {controlled: !!navigator.serviceWorker.controller, a: await waitImg(document.getElementById('a')), b: await waitImg(document.getElementById('b'))}", Duration::from_secs(5)));
    report("9", "add plain filter (document source only)", add_filter(&c1, &pat, false));
    OVERRIDE_HITS.store(0, Ordering::SeqCst);
    OVERRIDE_URIS.lock().unwrap().clear();
    nav(&c1, &format!("{SITE}/sw.html?plain"));
    report("9", "sw.html with plain filter (a: SW fetch passthrough, b: SW Cache Storage)", json!({
        "result": js_async(&c1, "return {controlled: !!navigator.serviceWorker.controller, a: await waitImg(document.getElementById('a')), b: await waitImg(document.getElementById('b'))}", Duration::from_secs(5)),
        "hits": OVERRIDE_URIS.lock().unwrap().clone(),
    }));
    remove_filters(&c1, &pat);
    report("9", "add filter with source kinds ALL (ICoreWebView2_22)", add_filter(&c1, &pat, true));
    OVERRIDE_URIS.lock().unwrap().clear();
    nav(&c1, &format!("{SITE}/sw.html?all"));
    report("9", "sw.html with ALL-source filter", json!({
        "result": js_async(&c1, "return {controlled: !!navigator.serviceWorker.controller, a: await waitImg(document.getElementById('a')), b: await waitImg(document.getElementById('b'))}", Duration::from_secs(5)),
        "hits": OVERRIDE_URIS.lock().unwrap().clone(),
    }));
    remove_filters(&c1, &pat);

    // ── 11번: 비밀번호 저장
    report("11", "set IsPasswordAutosaveEnabled/IsGeneralAutofillEnabled on c1", set_password_settings(&c1));

    // ── 12번: DRM
    let drm = r#"
      const cfg = [{ initDataTypes: ['cenc'], videoCapabilities: [{ contentType: 'video/mp4; codecs="avc1.42E01E"' }], audioCapabilities: [{ contentType: 'audio/mp4; codecs="mp4a.40.2"' }] }];
      const out = {};
      for (const ks of ['com.widevine.alpha', 'com.microsoft.playready', 'com.microsoft.playready.recommendation', 'com.microsoft.playready.recommendation.3000', 'org.w3.clearkey']) {
        try { const a = await navigator.requestMediaKeySystemAccess(ks, cfg); await a.createMediaKeys(); out[ks] = 'supported'; }
        catch (e) { out[ks] = 'no: ' + e.name + ' ' + e.message; }
      }
      out.userAgent = navigator.userAgent;
      return out;
    "#;
    nav(&c2, "https://example.com/?drm");
    report("12", "EME key systems (https page)", js_async(&c2, drm, Duration::from_secs(15)));

    // ── 13번: 압축 해제 확장
    let ext_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testext");
    report("13", "AddBrowserExtension(testext) result", add_extension(&c1, fs::canonicalize(&ext_dir).unwrap()));
    nav(&c1, &format!("{SITE}/?ext"));
    thread::sleep(Duration::from_millis(500));
    nav(&c1, &format!("{SITE}/?ext2"));
    report("13", "content script effect on localhost page", js(&c1, "document.documentElement.dataset.s0ext || null"));

    // ── 7번 준비: 단축키 처리기
    report("7", "AcceleratorKeyPressed handler install c1", install_accelerator_handler(&c1));
    report("7", "AcceleratorKeyPressed handler install c2", install_accelerator_handler(&c2));

    // ── 8번: TrySuspend + 메모리 (별도 탭 c3에 무거운 페이지)
    let win = app.get_window("main").unwrap();
    let (x, y, w, h) = *CARD.lock().unwrap();
    let c3 = win
        .add_child(content_builder("c3", "https://www.youtube.com/"), LogicalPosition::new(x, y), LogicalSize::new(w, h))
        .unwrap();
    wait_loaded(&c3, "youtube.com");
    thread::sleep(Duration::from_secs(8));
    js(&c3, "window.__s0count=0; window.__s0timer=setInterval(()=>window.__s0count++, 100); 1");
    report("8", "TrySuspend while c3 VISIBLE", try_suspend(&c3));
    report("8", "IsSuspended after visible attempt", is_suspended(&c3));
    let _ = resume(&c3);
    let _ = c3.hide();
    show_tab(app.clone(), "c1".into());
    thread::sleep(Duration::from_secs(3));
    let before = mem_snapshot(&c3);
    report("8", "memory: c3 hidden, awake", before);
    report("8", "TrySuspend after hide", try_suspend(&c3));
    report("8", "IsSuspended", is_suspended(&c3));
    let count_at_suspend = js(&c3, "window.__s0count");
    thread::sleep(Duration::from_secs(5));
    report("8", "memory: c3 suspended (+5s)", mem_snapshot(&c3));
    report("8", "Resume", resume(&c3));
    let count_after = js(&c3, "window.__s0count");
    report("8", "timer counter at suspend vs right after resume (5s gap; 100ms timer)", json!({"at_suspend": count_at_suspend, "after_resume": count_after}));
    thread::sleep(Duration::from_secs(2));
    report("8", "memory: c3 resumed (+2s)", mem_snapshot(&c3));
    report("8", "MemoryUsageTargetLevel=Low on hidden c3", set_mem_level(&c3, true));
    thread::sleep(Duration::from_secs(5));
    report("8", "memory: c3 hidden, target level Low (+5s)", mem_snapshot(&c3));
    set_mem_level(&c3, false);
    let _ = c3.close();
    thread::sleep(Duration::from_secs(2));
    report("8", "memory: after closing c3 webview (discard)", mem_snapshot(&c1));

    show_tab(app.clone(), "c1".into());
    nav(&c1, "https://example.com/");
    nav(&c2, "https://www.wikipedia.org/");
    report("-", "autorun finished", json!(true));
    AUTORUN_DONE.store(true, Ordering::SeqCst);
}

fn content_builder(label: &str, url: &str) -> WebviewBuilder<tauri::Wry> {
    WebviewBuilder::new(label, WebviewUrl::External(url.parse().unwrap()))
        .data_directory(out_dir().join("webview-data"))
        .browser_extensions_enabled(true)
        .focused(false)
}

// ───────────────────────── 외부 시험 스크립트용 명령 통로 ─────────────────────────

fn command_loop(app: AppHandle) {
    let cmd_file = out_dir().join("cmd.txt");
    let done_file = out_dir().join("cmd-done.txt");
    loop {
        thread::sleep(Duration::from_millis(150));
        let Ok(cmd) = fs::read_to_string(&cmd_file) else { continue };
        let _ = fs::remove_file(&cmd_file);
        let cmd = cmd.trim().to_string();
        let mut parts = cmd.splitn(3, ' ');
        let (a, b, c) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        let get = |l: &str| app.get_webview(l);
        let res: Value = match a {
            "status" => json!({"autorun_done": AUTORUN_DONE.load(Ordering::SeqCst)}),
            "show" => {
                show_tab(app.clone(), b.into());
                json!("ok")
            }
            "round" => {
                *ROUND.lock().unwrap() = b.parse().unwrap_or(12.0);
                apply_bounds(&app);
                json!("ok")
            }
            "nav" => json!(get(b).map(|w| nav(&w, c))),
            "js" => get(b).map(|w| js(&w, c)).unwrap_or(Value::Null),
            "jsasync" => get(b).map(|w| js_async(&w, c, Duration::from_secs(10))).unwrap_or(Value::Null),
            "focus" => get(b).map(|w| focus_content(&w)).unwrap_or(Value::Null),
            "capture" => get(b).map(|w| capture_preview(&w, out_dir().join(format!("capture-{b}.png")))).unwrap_or(Value::Null),
            "popup-child" => {
                let p = ensure_popup_child(&app);
                if b == "on" {
                    let (x, y, _, _) = *CARD.lock().unwrap();
                    let _ = p.set_position(LogicalPosition::new(x + 120.0, y - 4.0));
                    let _ = p.set_size(LogicalSize::new(360.0, 112.0));
                    let _ = p.show();
                } else {
                    let _ = p.hide();
                }
                json!("ok")
            }
            "popup-window" => {
                let w = match app.get_webview_window("popwin") {
                    Some(w) => w,
                    None => {
                        let main = app.get_window("main").unwrap();
                        WebviewWindowBuilder::new(&app, "popwin", WebviewUrl::App("popup.html".into()))
                            .data_directory(out_dir().join("webview-data"))
                            .browser_extensions_enabled(true)
                            .decorations(false)
                            .transparent(true)
                            .shadow(false)
                            .skip_taskbar(true)
                            .resizable(false)
                            .focused(false)
                            .visible(false)
                            .owner_raw(main.hwnd().unwrap())
                            .inner_size(392.0, 144.0)
                            .build()
                            .unwrap();
                        let pw = app.get_webview_window("popwin").unwrap();
                        event_log(&format!("popwin dev desktop move: {:?}", move_to_dev_desktop(pw.hwnd().unwrap().0 as isize)));
                        pw
                    }
                };
                if b == "on" || b == "on-noact" {
                    let main = app.get_window("main").unwrap();
                    let scale = main.scale_factor().unwrap();
                    let pos = main.inner_position().unwrap().to_logical::<f64>(scale);
                    let (x, y, _, _) = *CARD.lock().unwrap();
                    let _ = w.set_position(LogicalPosition::new(pos.x + x + 104.0, pos.y + y + 100.0));
                    if b == "on" {
                        let _ = w.show();
                    } else {
                        // 활성화 없이 보이기 (주소창 포커스를 지키기 위해)
                        let raw = w.hwnd().unwrap().0 as isize;
                        let _ = w.run_on_main_thread(move || unsafe {
                            let h = HWND(raw as _);
                            let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
                            SetWindowLongPtrW(h, GWL_EXSTYLE, ex | WS_EX_NOACTIVATE.0 as isize);
                            let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
                            let _ = SetWindowPos(h, Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                        });
                    }
                } else {
                    let _ = w.hide();
                }
                json!("ok")
            }
            "undecorate" => {
                // 자식 웹뷰를 다 만든 뒤 장식을 끄면 Tauri가 크기 조절용 테두리 창을 붙인다(아래 메모 참고)
                let w = app.get_window("main").unwrap();
                let _ = w.set_decorations(true);
                let _ = w.set_decorations(false);
                json!("ok")
            }
            "snap" => {
                place_snap_overlay(&app);
                json!("ok")
            }
            "filter-add" => {
                // filter-add c1 plain|all pattern
                let mut it = c.splitn(2, ' ');
                let (mode, pat) = (it.next().unwrap_or("plain"), it.next().unwrap_or(""));
                get(b).map(|w| add_filter(&w, pat, mode == "all")).unwrap_or(Value::Null)
            }
            "filter-rm" => {
                if let Some(w) = get(b) { remove_filters(&w, c); }
                json!("ok")
            }
            "handler" => get(b).map(|w| install_override_handler(&w)).unwrap_or(Value::Null),
            "hits" => {
                let v = json!({"count": OVERRIDE_HITS.load(Ordering::SeqCst), "uris": OVERRIDE_URIS.lock().unwrap().clone()});
                OVERRIDE_HITS.store(0, Ordering::SeqCst);
                OVERRIDE_URIS.lock().unwrap().clear();
                v
            }
            "webviews" => json!(app.webviews().keys().cloned().collect::<Vec<_>>()),
            "close" => json!(get(b).map(|w| format!("{:?}", w.close()))),
            "env" => get(b).map(|w| json!({"env": env_ptr(&w), "browser_pid": browser_pid(&w)})).unwrap_or(Value::Null),
            "shared-env" => {
                // c1의 환경 객체를 그대로 넘겨 c4를 만든다 (WebviewBuilder::with_environment)
                let c1 = get("c1").unwrap();
                let win = app.get_window("main").unwrap();
                let r = on_main(&c1, Duration::from_secs(20), move |p, tx| {
                    let env = p.environment();
                    let b = content_builder("c4", "https://example.com/?c4").with_environment(env);
                    let r = win.add_child(b, LogicalPosition::new(300.0, 300.0), LogicalSize::new(300.0, 200.0));
                    let _ = tx.send(format!("{:?}", r.map(|w| w.label().to_string())));
                });
                thread::sleep(Duration::from_secs(2));
                json!({"create": r, "c1_env": get("c1").map(|w| env_ptr(&w)), "c4_env": get("c4").map(|w| env_ptr(&w))})
            }
            "clear-cache" => get(b).map(|w| clear_cache(&w)).unwrap_or(Value::Null),
            "popwin-state" => json!(app.get_webview_window("popwin").map(|w| format!("visible={:?} pos={:?} size={:?}", w.is_visible(), w.outer_position(), w.outer_size()))),
            "accel" => get(b).map(|w| install_accelerator_handler(&w)).unwrap_or(Value::Null),
            "native-close" => get(b).map(|w| {
                // 웹뷰를 버리기 전에 컨트롤러를 직접 닫는다 (ICoreWebView2Controller::Close)
                let r = on_main(&w, Duration::from_secs(5), |p, tx| unsafe {
                    let _ = tx.send(format!("{:?}", p.controller().Close()));
                });
                let c = w.close();
                json!({"controller_close": r, "tauri_close": format!("{c:?}")})
            }).unwrap_or(Value::Null),
            "pw" => get(b).map(|w| set_password_settings(&w)).unwrap_or(Value::Null),
            "mem" => get("c1").map(|w| mem_snapshot(&w)).unwrap_or(Value::Null),
            "hwnd" => {
                let w = app.get_window("main").unwrap();
                let r = w.outer_position().ok().zip(w.outer_size().ok());
                json!({"hwnd": format!("{:?}", w.hwnd().map(|h| h.0 as isize)), "rect": format!("{r:?}"), "maximized": w.is_maximized().ok(), "scale": w.scale_factor().ok()})
            }
            "" => continue,
            _ => json!(format!("unknown command {a}")),
        };
        let _ = fs::write(&done_file, json!({"cmd": cmd, "result": res}).to_string());
    }
}

fn main() {
    // TBS_S0_OUT으로 기록·데이터 폴더를 바꿀 수 있다 (개발용 인스턴스를 사용자 인스턴스와 분리)
    let out = std::env::var("TBS_S0_OUT").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out"));
    fs::create_dir_all(&out).unwrap();
    let _ = fs::remove_file(out.join("report.jsonl"));
    let _ = fs::remove_file(out.join("events.log"));
    OUT.set(out).unwrap();
    let data_dir = out_dir().join("webview-data");

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![s0_ping, set_content_bounds, show_tab, ui_log])
        .setup(move |app| {
            let handle = app.handle().clone();
            APP.set(handle.clone()).unwrap();
            let win = WindowBuilder::new(app, "main")
                .title(APP_NAME)
                .inner_size(1280.0, 800.0)
                .min_inner_size(640.0, 400.0)
                .decorations(false)
                .visible(false)
                .focused(std::env::var("TBS_DEV_DESKTOP").is_err())
                .build()?;
            let size = win.inner_size()?.to_logical::<f64>(win.scale_factor()?);
            // UI 웹뷰: 창 전체
            win.add_child(
                WebviewBuilder::new("ui", WebviewUrl::App("index.html".into()))
                    .data_directory(data_dir.clone())
                    .browser_extensions_enabled(true)
                    .focused(std::env::var("TBS_DEV_DESKTOP").is_err())
                    .auto_resize(),
                LogicalPosition::new(0.0, 0.0),
                LogicalSize::new(size.width, size.height),
            )?;
            // 콘텐츠 웹뷰 두 개: 카드 자리(처음엔 임시 위치, UI가 좌표를 알려 주면 옮긴다)
            let c1 = win.add_child(content_builder("c1", "https://example.com/"), LogicalPosition::new(276.0, 50.0), LogicalSize::new(900.0, 700.0))?;
            let c2 = win.add_child(content_builder("c2", "https://www.wikipedia.org/"), LogicalPosition::new(276.0, 50.0), LogicalSize::new(900.0, 700.0))?;
            let _ = c2.hide();
            let _ = c1;
            install_snap_overlay(&handle);
            // 숨긴 채로 만든 뒤 (필요하면 개발용 데스크톱으로 옮기고) 활성화 없이 보인다
            let main_raw = win.hwnd()?.0 as isize;
            let moved = move_to_dev_desktop(main_raw);
            unsafe {
                let _ = ShowWindow(HWND(main_raw as _), SW_SHOWNA);
            }
            event_log(&format!("main window shown (dev desktop move: {moved:?})"));
            {
                let h = handle.clone();
                win.on_window_event(move |e| {
                    if let tauri::WindowEvent::Resized(_) = e {
                        place_snap_overlay(&h);
                    }
                    if let tauri::WindowEvent::Moved(p) = e {
                        event_log(&format!("window moved {p:?}"));
                    }
                    if let tauri::WindowEvent::Resized(s) = e {
                        event_log(&format!("window resized {s:?}"));
                    }
                });
            }
            let h2 = handle.clone();
            thread::spawn(move || command_loop(h2));
            if std::env::var("S0_AUTORUN").is_ok() {
                thread::spawn(move || autorun(handle));
            } else {
                // 사용자 시험 준비: 위쪽 변 크기 조절, 스냅 레이아웃 덮개, 단축키 처리기, 비밀번호 저장
                thread::spawn(move || {
                    thread::sleep(Duration::from_millis(1500));
                    let w = handle.get_window("main").unwrap();
                    let _ = w.set_decorations(true);
                    let _ = w.set_decorations(false);
                    thread::sleep(Duration::from_millis(300));
                    place_snap_overlay(&handle);
                    for l in ["c1", "c2"] {
                        if let Some(wv) = handle.get_webview(l) {
                            install_accelerator_handler(&wv);
                            set_password_settings(&wv);
                        }
                    }
                    event_log("준비 완료: 탭 1/탭 2를 눌러 번갈아 보기, 페이지 안에서 Ctrl+T 등을 누르면 여기에 기록된다");
                });
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("실행 실패");
}
