//! Tauri가 감싸지 않은 WebView2 기능을 webview2-com으로 직접 부른다.
//! 여기 있는 함수는 모두 메인 스레드에서만 부른다.

use std::rc::Rc;

use webview2_com::{Microsoft::Web::WebView2::Win32::*, *};
use windows::{
    core::{Interface, BOOL, PWSTR},
    Win32::System::Com::{IStream, STREAM_SEEK_SET},
};

/// 메인 스레드에서만 쓰는 값을 Tauri의 `Send` 요구를 넘겨 메인 스레드 작업으로 옮길 때 쓴다.
/// 안에 든 COM 객체는 메인 스레드 밖에서 건드리지 않는다.
pub struct MainOnly<T>(pub T);
unsafe impl<T> Send for MainOnly<T> {}

/// 콘텐츠 웹뷰에서 호스트로 올라오는 일.
pub enum Event {
    /// 주소가 바뀌었다. 두 번째 값은 문서를 새로 불러오지 않은 이동인지 (pushState 등)
    Url(String, bool),
    /// 이동이 끝났다 (성공했는지)
    Completed(bool),
    /// 페이지에 포커스가 있을 때 누른 브라우저 단축키 (spec 5.6)
    Shortcut(&'static str),
    History { back: bool, forward: bool },
    Loading(bool),
    Title(String),
    Favicon(Option<String>),
    NewWindow { uri: String, args: ICoreWebView2NewWindowRequestedEventArgs, deferral: ICoreWebView2Deferral },
    /// 페이지가 `window.close()`로 스스로 닫히려 한다 (스크립트로 연 팝업 탭)
    CloseRequested,
    /// 소리를 내는지 바뀌었다 (spec 4.5 재생 표시, spec 7장 예외)
    Audio(bool),
    /// 페이지가 보낸 메시지 (사이트 스크립트의 부탁일 수 있다, spec 6.3). (보낸 페이지 주소, 내용)
    Message(String, String),
    /// 페이지(영상 등)가 전체 화면을 켰거나 껐다 (spec 5.5)
    FullScreen(bool),
    /// 카메라, 마이크, 알림, 위치 권한을 요청했다 (spec 5.5). 다른 권한은 WebView2 기본 동작에 맡긴다.
    Permission { uri: String, kind: &'static str, args: ICoreWebView2PermissionRequestedEventArgs },
    /// 페이지가 포커스를 받았다 (페이지를 눌렀다). 떠 있는 팝업을 닫는다.
    GotFocus,
    /// 오른쪽 클릭 메뉴를 띄우려 한다. 확장 항목을 더할 수 있다 (처리기 안에서 바로 고친다)
    ContextMenu(ICoreWebView2ContextMenuRequestedEventArgs),
}

#[derive(Clone, Copy)]
enum Kind {
    Source,
    History,
    NavStart,
    NavDone,
    Title,
    Favicon,
    NewWindow,
    WindowClose,
    Accelerator,
    Message,
    Audio,
    FullScreen,
    Permission,
    GotFocus,
    ContextMenu,
}

/// 웹뷰 하나에 건 처리기들. 웹뷰를 닫기 전에 `unhook`으로 뗀다 (spec 7장).
pub struct Hooks {
    pub core: ICoreWebView2,
    pub controller: ICoreWebView2Controller,
    tokens: Vec<(Kind, i64)>,
}

pub fn source(core: &ICoreWebView2) -> String {
    let mut s = PWSTR::null();
    unsafe {
        let _ = core.Source(&mut s);
    }
    take_pwstr(s)
}

pub fn title(core: &ICoreWebView2) -> String {
    let mut s = PWSTR::null();
    unsafe {
        let _ = core.DocumentTitle(&mut s);
    }
    take_pwstr(s)
}

pub fn history(core: &ICoreWebView2) -> (bool, bool) {
    let (mut b, mut f) = (BOOL(0), BOOL(0));
    unsafe {
        let _ = core.CanGoBack(&mut b);
        let _ = core.CanGoForward(&mut f);
    }
    (b.as_bool(), f.as_bool())
}

/// 비밀번호 저장과 자동 입력을 켠다. 웹뷰마다 따로라 만들 때마다 부른다 (0단계 11번).
pub fn enable_autofill(core: &ICoreWebView2) {
    unsafe {
        if let Ok(s4) = core.Settings().and_then(|s| s.cast::<ICoreWebView2Settings4>()) {
            let _ = s4.SetIsPasswordAutosaveEnabled(true);
            let _ = s4.SetIsGeneralAutofillEnabled(true);
        }
    }
}

pub fn hook(controller: &ICoreWebView2Controller, core: &ICoreWebView2, sink: Rc<dyn Fn(Event)>) -> windows::core::Result<Hooks> {
    let mut tokens = Vec::new();
    unsafe {
        let s = sink.clone();
        let mut t = 0i64;
        core.add_SourceChanged(
            &SourceChangedEventHandler::create(Box::new(move |wv, args| {
                if let Some(wv) = wv {
                    let mut new_doc = BOOL(1);
                    if let Some(a) = args {
                        let _ = a.IsNewDocument(&mut new_doc);
                    }
                    s(Event::Url(source(&wv), !new_doc.as_bool()));
                }
                Ok(())
            })),
            &mut t,
        )?;
        tokens.push((Kind::Source, t));

        let s = sink.clone();
        core.add_HistoryChanged(
            &HistoryChangedEventHandler::create(Box::new(move |wv, _| {
                if let Some(wv) = wv {
                    let (back, forward) = history(&wv);
                    s(Event::History { back, forward });
                }
                Ok(())
            })),
            &mut t,
        )?;
        tokens.push((Kind::History, t));

        let s = sink.clone();
        core.add_NavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(move |_, _| {
                s(Event::Loading(true));
                Ok(())
            })),
            &mut t,
        )?;
        tokens.push((Kind::NavStart, t));

        let s = sink.clone();
        core.add_NavigationCompleted(
            &NavigationCompletedEventHandler::create(Box::new(move |wv, args| {
                s(Event::Loading(false));
                let mut ok = BOOL(0);
                if let Some(a) = args {
                    let _ = a.IsSuccess(&mut ok);
                }
                s(Event::Completed(ok.as_bool()));
                if let Some(wv) = wv {
                    let (back, forward) = history(&wv);
                    s(Event::History { back, forward });
                }
                Ok(())
            })),
            &mut t,
        )?;
        tokens.push((Kind::NavDone, t));

        let s = sink.clone();
        core.add_DocumentTitleChanged(
            &DocumentTitleChangedEventHandler::create(Box::new(move |wv, _| {
                if let Some(wv) = wv {
                    s(Event::Title(title(&wv)));
                }
                Ok(())
            })),
            &mut t,
        )?;
        tokens.push((Kind::Title, t));

        if let Ok(c15) = core.cast::<ICoreWebView2_15>() {
            let s = sink.clone();
            c15.add_FaviconChanged(
                &FaviconChangedEventHandler::create(Box::new(move |wv, _| {
                    if let Some(wv) = wv {
                        fetch_favicon(&wv, s.clone());
                    }
                    Ok(())
                })),
                &mut t,
            )?;
            tokens.push((Kind::Favicon, t));
        }

        let s = sink.clone();
        core.add_NewWindowRequested(
            &NewWindowRequestedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut uri = PWSTR::null();
                args.Uri(&mut uri)?;
                let deferral = args.GetDeferral()?;
                s(Event::NewWindow { uri: take_pwstr(uri), args, deferral });
                Ok(())
            })),
            &mut t,
        )?;
        tokens.push((Kind::NewWindow, t));

        let s = sink.clone();
        core.add_WindowCloseRequested(
            &WindowCloseRequestedEventHandler::create(Box::new(move |_, _| {
                s(Event::CloseRequested);
                Ok(())
            })),
            &mut t,
        )?;
        tokens.push((Kind::WindowClose, t));

        let s = sink.clone();
        controller.add_AcceleratorKeyPressed(
            &AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
                let mut vk = 0u32;
                args.KeyEventKind(&mut kind)?;
                args.VirtualKey(&mut vk)?;
                if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN && kind != COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN {
                    return Ok(());
                }
                use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
                let down = |k: i32| GetKeyState(k) < 0;
                if let Some(name) = crate::shortcuts::name(vk, down(0x11), down(0x10), down(0x12)) {
                    // 브라우저 단축키는 페이지로 넘기지 않는다
                    args.SetHandled(true)?;
                    s(Event::Shortcut(name));
                }
                Ok(())
            })),
            &mut t,
        )?;
        tokens.push((Kind::Accelerator, t));

        let s = sink.clone();
        core.add_WebMessageReceived(
            &WebMessageReceivedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut src = PWSTR::null();
                args.Source(&mut src)?;
                let mut text = PWSTR::null();
                // 문자열이 아닌 메시지(Tauri 내부 메시지 등)는 무시한다
                if args.TryGetWebMessageAsString(&mut text).is_ok() {
                    s(Event::Message(take_pwstr(src), take_pwstr(text)));
                }
                Ok(())
            })),
            &mut t,
        )?;
        tokens.push((Kind::Message, t));

        if let Ok(c8) = core.cast::<ICoreWebView2_8>() {
            let s = sink.clone();
            c8.add_IsDocumentPlayingAudioChanged(
                &IsDocumentPlayingAudioChangedEventHandler::create(Box::new(move |wv, _| {
                    if let Some(c8) = wv.and_then(|w| w.cast::<ICoreWebView2_8>().ok()) {
                        let mut playing = BOOL(0);
                        let _ = c8.IsDocumentPlayingAudio(&mut playing);
                        s(Event::Audio(playing.as_bool()));
                    }
                    Ok(())
                })),
                &mut t,
            )?;
            tokens.push((Kind::Audio, t));
        }

        let s = sink.clone();
        core.add_ContainsFullScreenElementChanged(
            &ContainsFullScreenElementChangedEventHandler::create(Box::new(move |wv, _| {
                if let Some(wv) = wv {
                    let mut on = BOOL(0);
                    let _ = wv.ContainsFullScreenElement(&mut on);
                    s(Event::FullScreen(on.as_bool()));
                }
                Ok(())
            })),
            &mut t,
        )?;
        tokens.push((Kind::FullScreen, t));

        let s = sink.clone();
        core.add_PermissionRequested(
            &PermissionRequestedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut kind = COREWEBVIEW2_PERMISSION_KIND::default();
                args.PermissionKind(&mut kind)?;
                let kind = match kind {
                    COREWEBVIEW2_PERMISSION_KIND_CAMERA => "camera",
                    COREWEBVIEW2_PERMISSION_KIND_MICROPHONE => "microphone",
                    COREWEBVIEW2_PERMISSION_KIND_NOTIFICATIONS => "notifications",
                    COREWEBVIEW2_PERMISSION_KIND_GEOLOCATION => "geolocation",
                    _ => return Ok(()),
                };
                // 고른 값은 우리가 settings.json에 기억한다. WebView2 프로필에는 남기지 않는다 (지우면 다시 묻도록).
                if let Ok(a3) = args.cast::<ICoreWebView2PermissionRequestedEventArgs3>() {
                    let _ = a3.SetSavesInProfile(false);
                }
                let mut uri = PWSTR::null();
                args.Uri(&mut uri)?;
                s(Event::Permission { uri: take_pwstr(uri), kind, args });
                Ok(())
            })),
            &mut t,
        )?;
        tokens.push((Kind::Permission, t));

        let s = sink.clone();
        controller.add_GotFocus(
            &FocusChangedEventHandler::create(Box::new(move |_, _| {
                s(Event::GotFocus);
                Ok(())
            })),
            &mut t,
        )?;
        tokens.push((Kind::GotFocus, t));

        if let Ok(c11) = core.cast::<ICoreWebView2_11>() {
            let s = sink.clone();
            c11.add_ContextMenuRequested(
                &ContextMenuRequestedEventHandler::create(Box::new(move |_, args| {
                    if let Some(a) = args {
                        s(Event::ContextMenu(a));
                    }
                    Ok(())
                })),
                &mut t,
            )?;
            tokens.push((Kind::ContextMenu, t));
        }
    }
    Ok(Hooks { core: core.clone(), controller: controller.clone(), tokens })
}

/// 권한 요청에 답한다. 처리기 안에서 바로 답하거나, 미뤄 둔 요청이면 `deferral`을 끝낸다.
pub fn answer_permission(args: &ICoreWebView2PermissionRequestedEventArgs, allow: Option<bool>) {
    let state = match allow {
        Some(true) => COREWEBVIEW2_PERMISSION_STATE_ALLOW,
        Some(false) => COREWEBVIEW2_PERMISSION_STATE_DENY,
        None => COREWEBVIEW2_PERMISSION_STATE_DEFAULT,
    };
    unsafe {
        let _ = args.SetState(state);
    }
}

/// 페이지의 전체 화면을 끈다 (다른 탭으로 가거나 F11을 누를 때)
pub fn exit_fullscreen(core: &ICoreWebView2) {
    unsafe {
        let _ = core.ExecuteScript(
            windows::core::w!("document.fullscreenElement && document.exitFullscreen()"),
            &ExecuteScriptCompletedHandler::create(Box::new(|_, _| Ok(()))),
        );
    }
}

/// 페이지가 바탕을 칠하기 전에 보이는 빈 바탕. 다크 테마에서 흰 화면이 번쩍이지 않게 UI가 알려 준 카드 색과 맞춘다.
pub fn set_background(controller: &ICoreWebView2Controller, (r, g, b): (u8, u8, u8)) {
    unsafe {
        if let Ok(c2) = controller.cast::<ICoreWebView2Controller2>() {
            let _ = c2.SetDefaultBackgroundColor(COREWEBVIEW2_COLOR { A: 255, R: r, G: g, B: b });
        }
    }
}

/// 웹 페이지가 보는 prefers-color-scheme (프로필 전체에 걸린다). "light" | "dark" | 그 밖은 Windows 설정을 따른다.
pub fn set_color_scheme(core: &ICoreWebView2, scheme: &str) {
    let v = match scheme {
        "light" => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_LIGHT,
        "dark" => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_DARK,
        _ => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_AUTO,
    };
    unsafe {
        if let Ok(p) = core.cast::<ICoreWebView2_13>().and_then(|c| c.Profile()) {
            let _ = p.SetPreferredColorScheme(v);
        }
    }
}

pub fn unhook(h: &Hooks) {
    unsafe {
        for &(kind, t) in &h.tokens {
            let _ = match kind {
                Kind::Source => h.core.remove_SourceChanged(t),
                Kind::History => h.core.remove_HistoryChanged(t),
                Kind::NavStart => h.core.remove_NavigationStarting(t),
                Kind::NavDone => h.core.remove_NavigationCompleted(t),
                Kind::Title => h.core.remove_DocumentTitleChanged(t),
                Kind::Favicon => h.core.cast::<ICoreWebView2_15>().and_then(|c| c.remove_FaviconChanged(t)),
                Kind::NewWindow => h.core.remove_NewWindowRequested(t),
                Kind::WindowClose => h.core.remove_WindowCloseRequested(t),
                Kind::Accelerator => h.controller.remove_AcceleratorKeyPressed(t),
                Kind::Message => h.core.remove_WebMessageReceived(t),
                Kind::Audio => h.core.cast::<ICoreWebView2_8>().and_then(|c| c.remove_IsDocumentPlayingAudioChanged(t)),
                Kind::FullScreen => h.core.remove_ContainsFullScreenElementChanged(t),
                Kind::Permission => h.core.remove_PermissionRequested(t),
                Kind::GotFocus => h.controller.remove_GotFocus(t),
                Kind::ContextMenu => h.core.cast::<ICoreWebView2_11>().and_then(|c| c.remove_ContextMenuRequested(t)),
            };
        }
    }
}

fn fetch_favicon(core: &ICoreWebView2, sink: Rc<dyn Fn(Event)>) {
    unsafe {
        let Ok(c15) = core.cast::<ICoreWebView2_15>() else { return };
        let mut uri = PWSTR::null();
        let _ = c15.FaviconUri(&mut uri);
        if take_pwstr(uri).is_empty() {
            sink(Event::Favicon(None));
            return;
        }
        let handler = GetFaviconCompletedHandler::create(Box::new(move |err, stream| {
            let png = if err.is_ok() { stream.and_then(|s| read_stream(&s)) } else { None };
            sink(Event::Favicon(png.filter(|b| !b.is_empty()).map(|b| format!("data:image/png;base64,{}", base64(&b)))));
            Ok(())
        }));
        let _ = c15.GetFavicon(COREWEBVIEW2_FAVICON_IMAGE_FORMAT_PNG, &handler);
    }
}

pub fn read_stream(s: &IStream) -> Option<Vec<u8>> {
    unsafe {
        s.Seek(0, STREAM_SEEK_SET, None).ok()?;
        let mut out = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            let mut read = 0u32;
            if s.Read(buf.as_mut_ptr() as _, buf.len() as u32, Some(&mut read)).is_err() {
                return None;
            }
            if read == 0 {
                break;
            }
            out.extend_from_slice(&buf[..read as usize]);
        }
        Some(out)
    }
}

pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        } as u32)
    }
    let clean: Vec<u8> = text.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=').collect();
    let mut out = Vec::with_capacity(clean.len() * 3 / 4);
    for chunk in clean.chunks(4) {
        if chunk.len() == 1 {
            return None;
        }
        let mut n = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            n |= val(*c)? << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_decodes() {
        assert_eq!(super::base64_decode("Zm9vYmFy").unwrap(), b"foobar");
        assert_eq!(super::base64_decode("Zm8=").unwrap(), b"fo");
        assert_eq!(super::base64_decode("Zg==").unwrap(), b"f");
        assert!(super::base64_decode("Z").is_none());
        assert!(super::base64_decode("@@@@").is_none());
        let data: Vec<u8> = (0..=255).collect();
        assert_eq!(super::base64_decode(&super::base64(&data)).unwrap(), data);
    }

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(super::base64(b""), "");
        assert_eq!(super::base64(b"f"), "Zg==");
        assert_eq!(super::base64(b"fo"), "Zm8=");
        assert_eq!(super::base64(b"foo"), "Zm9v");
        assert_eq!(super::base64(b"foobar"), "Zm9vYmFy");
    }
}
