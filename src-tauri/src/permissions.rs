//! 웹 페이지의 권한 요청 (spec 5.5): 카메라, 마이크, 알림, 위치.
//! 사이트별로 기억한 답이 있으면 바로 답하고, 없으면 UI에 묻는다. 고른 답은 settings.json에 남는다 (spec 8장).
//! 묻는 동안 페이지는 기다린다 (WebView2 deferral). 여기 있는 함수는 모두 메인 스레드에서만 부른다.

use std::cell::RefCell;

use serde::Serialize;
use tauri::{AppHandle, Url};
use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2Deferral, ICoreWebView2PermissionRequestedEventArgs};

use crate::{native, settings, state::TabId};

struct Pending {
    tab: TabId,
    site: String,
    kind: &'static str,
    args: ICoreWebView2PermissionRequestedEventArgs,
    deferral: ICoreWebView2Deferral,
}

thread_local! {
    static PENDING: RefCell<Vec<Pending>> = const { RefCell::new(Vec::new()) };
}

/// UI가 보여 줄 질문 하나: 한 탭의 한 사이트가 요청한 권한들
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub tab: TabId,
    pub site: String,
    pub kinds: Vec<&'static str>,
}

/// "https://meet.example.com/abc" → "https://meet.example.com"
pub fn site_of(uri: &str) -> String {
    match Url::parse(uri) {
        Ok(u) => u.origin().ascii_serialization(),
        Err(_) => uri.to_string(),
    }
}

/// 처리기 안에서 부른다 (deferral은 처리기 안에서만 받을 수 있다)
pub fn request(app: &AppHandle, tab: TabId, uri: &str, kind: &'static str, args: ICoreWebView2PermissionRequestedEventArgs) {
    let site = site_of(uri);
    if let Some(allow) = settings::permission(app, &site, kind) {
        native::answer_permission(&args, Some(allow));
        return;
    }
    let Ok(deferral) = (unsafe { args.GetDeferral() }) else { return };
    PENDING.with(|p| p.borrow_mut().push(Pending { tab, site, kind, args, deferral }));
    emit(app);
}

/// 사용자가 답했다. `remember`가 거짓이면 이번에만 (알림 닫기 = 이번만 차단).
pub fn answer(app: &AppHandle, tab: TabId, site: &str, allow: bool, remember: bool) -> Result<(), String> {
    let done = take(|p| p.tab == tab && p.site == site);
    for p in &done {
        if remember {
            settings::remember_permission(app, &p.site, p.kind, allow)?;
        }
        finish(p, Some(allow));
    }
    emit(app);
    Ok(())
}

/// 탭이 다른 문서로 가거나 웹뷰가 닫히면, 그 탭이 기다리던 요청은 거절로 끝낸다 (기억하지 않는다)
pub fn drop_tab(app: &AppHandle, tab: TabId) {
    let done = take(|p| p.tab == tab);
    if done.is_empty() {
        return;
    }
    for p in &done {
        finish(p, Some(false));
    }
    emit(app);
}

pub fn list() -> Vec<Request> {
    PENDING.with(|p| group(p.borrow().iter().map(|x| (x.tab, x.site.as_str(), x.kind))))
}

/// 탭마다 남겨 둔 것의 (이름, 탭 id). 개발용 `orphans` 확인에 쓴다
#[cfg(debug_assertions)]
pub fn tab_entries() -> Vec<(&'static str, TabId)> {
    PENDING.with(|p| p.borrow().iter().map(|x| ("permissions.PENDING", x.tab)).collect())
}

fn take(pred: impl Fn(&Pending) -> bool) -> Vec<Pending> {
    PENDING.with(|p| {
        let mut all = p.borrow_mut();
        let (hit, keep): (Vec<_>, Vec<_>) = all.drain(..).partition(|x| pred(x));
        *all = keep;
        hit
    })
}

fn finish(p: &Pending, allow: Option<bool>) {
    native::answer_permission(&p.args, allow);
    unsafe {
        let _ = p.deferral.Complete();
    }
}

fn emit(app: &AppHandle) {
    crate::browser::emit_ui(app, "permission_requests", list());
}

/// 같은 탭, 같은 사이트의 요청을 한 질문으로 묶는다 (카메라와 마이크를 함께 묻는 경우가 많다)
fn group<'a>(items: impl Iterator<Item = (TabId, &'a str, &'static str)>) -> Vec<Request> {
    let mut out: Vec<Request> = Vec::new();
    for (tab, site, kind) in items {
        match out.iter_mut().find(|r| r.tab == tab && r.site == site) {
            Some(r) => {
                if !r.kinds.contains(&kind) {
                    r.kinds.push(kind)
                }
            }
            None => out.push(Request { tab, site: site.to_string(), kinds: vec![kind] }),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_is_origin() {
        assert_eq!(site_of("https://meet.example.com/abc?x=1"), "https://meet.example.com");
        assert_eq!(site_of("http://localhost:8080/a"), "http://localhost:8080");
    }

    #[test]
    fn requests_group_by_tab_and_site() {
        let g = group(
            [(1, "https://a", "camera"), (1, "https://a", "microphone"), (2, "https://a", "camera"), (1, "https://a", "camera")]
                .into_iter(),
        );
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].kinds, vec!["camera", "microphone"]);
        assert_eq!(g[1].tab, 2);
    }
}
