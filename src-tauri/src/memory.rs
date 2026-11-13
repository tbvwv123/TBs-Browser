//! 메모리 관리 (spec 7장). 열린 탭 수가 아니라 깨어 있는 탭 수를 줄인다.
//!
//! | 상태 | 들어가는 조건 | 하는 일 |
//! | 백그라운드 | 다른 탭으로 넘어간 직후 | 숨기고 메모리 목표 수준을 Low로 |
//! | 재움 | 안 본 지 N분 (기본 5) | 목표 수준을 Normal로 되돌리고 TrySuspend (둘을 섞어 쓰지 않는다) |
//! | 버림 | 안 본 지 M분 (기본 30), 또는 깨어 있는 탭이 K개(기본 12)를 넘을 때 오래된 것부터 | 웹뷰를 닫는다. 주소·제목·파비콘만 남는다 |
//!
//! 소리를 내는 탭, 다운로드 중인 탭, 전체 화면 탭, "항상 유지" 탭은 재우거나 버리지 않는다.
//! 이 파일의 함수는 메인 스레드에서만 부른다.

use std::{
    cell::RefCell,
    collections::HashMap,
    time::{Duration, Instant},
};

use tauri::AppHandle;
use webview2_com::{Microsoft::Web::WebView2::Win32::*, *};
use windows::core::Interface;

use crate::{browser, downloads, settings, state::TabId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Awake,
    Background,
    Suspended,
}

#[derive(Clone, Copy, Debug)]
struct Mem {
    phase: Phase,
    /// 마지막으로 숨긴 때 (활성 탭이면 None)
    hidden_since: Option<Instant>,
    /// TrySuspend를 부르고 답을 기다리는 중
    suspending: bool,
}

thread_local! {
    static MEM: RefCell<HashMap<TabId, Mem>> = RefCell::new(HashMap::new());
}

/// 정책을 몇 초마다 볼지
const TICK: Duration = Duration::from_secs(20);

pub fn start(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(TICK);
        let a = app.clone();
        if app.run_on_main_thread(move || tick(&a)).is_err() {
            return;
        }
    });
}

fn set_level(core: &ICoreWebView2, low: bool) {
    let level = if low { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW } else { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL };
    if let Ok(c) = core.cast::<ICoreWebView2_19>() {
        let _ = unsafe { c.SetMemoryUsageTargetLevel(level) };
    }
}

/// 활성 탭이 바뀌었다. 전 탭은 백그라운드로, 새 탭은 깨운다.
pub fn on_switch(prev: Option<TabId>, next: Option<TabId>) {
    if let Some(p) = prev.filter(|p| Some(*p) != next) {
        if let Some(core) = browser::core_of(p) {
            set_level(&core, true);
            MEM.with(|m| {
                m.borrow_mut().insert(p, Mem { phase: Phase::Background, hidden_since: Some(Instant::now()), suspending: false });
            });
        }
    }
    if let Some(n) = next {
        wake(n);
    }
}

/// 다시 볼 때: 재운 탭은 Resume, 목표 수준은 Normal (spec 7장 "다시 볼 때")
fn wake(id: TabId) {
    let Some(core) = browser::core_of(id) else { return };
    let was = MEM.with(|m| m.borrow().get(&id).map(|x| x.phase));
    if was == Some(Phase::Suspended) {
        if let Ok(c3) = core.cast::<ICoreWebView2_3>() {
            let _ = unsafe { c3.Resume() };
        }
    }
    set_level(&core, false);
    MEM.with(|m| {
        m.borrow_mut().insert(id, Mem { phase: Phase::Awake, hidden_since: None, suspending: false });
    });
}

pub fn forget(id: TabId) {
    MEM.with(|m| m.borrow_mut().remove(&id));
}

/// 재우거나 버리면 안 되는 탭인지
fn exempt(app: &AppHandle, id: TabId) -> bool {
    let snap = browser::snapshot(app);
    let Some(t) = snap.tabs.iter().find(|t| t.id == id) else { return true };
    t.audible || t.keep_alive || downloads::tab_busy(id) || (browser::is_fullscreen(app) && snap.active == Some(id))
}

fn tick(app: &AppHandle) {
    let policy = settings::get(app).memory;
    let suspend_after = Duration::from_secs(u64::from(policy.suspend_after_minutes.max(1)) * 60);
    let discard_after = Duration::from_secs(u64::from(policy.discard_after_minutes.max(1)) * 60);
    let active = browser::snapshot(app).active;
    let now = Instant::now();

    // 웹뷰가 있는 탭들
    let loaded: Vec<TabId> = browser::loaded_tabs();
    for id in &loaded {
        if Some(*id) == active || exempt(app, *id) {
            continue;
        }
        let Some(m) = MEM.with(|m| m.borrow().get(id).copied()) else { continue };
        let Some(since) = m.hidden_since else { continue };
        let hidden = now.duration_since(since);
        if hidden >= discard_after {
            discard(app, *id);
        } else if hidden >= suspend_after && m.phase == Phase::Background && !m.suspending {
            suspend(*id);
        }
    }

    // 깨어 있는 탭이 너무 많으면 가장 오래 안 본 것부터 버린다
    let max = policy.max_awake_tabs.max(1) as usize;
    let mut awake: Vec<(TabId, Option<Instant>)> = browser::loaded_tabs()
        .into_iter()
        .map(|id| (id, MEM.with(|m| m.borrow().get(&id).copied())))
        .filter(|(_, m)| m.map_or(true, |m| m.phase != Phase::Suspended))
        .map(|(id, m)| (id, m.and_then(|m| m.hidden_since)))
        .collect();
    if awake.len() > max {
        awake.sort_by_key(|(_, since)| since.unwrap_or(now));
        let mut extra = awake.len() - max;
        for (id, since) in awake {
            if extra == 0 {
                break;
            }
            if since.is_none() || Some(id) == active || exempt(app, id) {
                continue;
            }
            discard(app, id);
            extra -= 1;
        }
    }
}

/// 재우기: 숨긴 뒤에 부르고, 실패하면 다음 주기에 다시 한다 (spec 7장)
fn suspend(id: TabId) {
    let Some(core) = browser::core_of(id) else { return };
    let Ok(c3) = core.cast::<ICoreWebView2_3>() else { return };
    // 목표 수준 낮추기와 섞지 않는다: TrySuspend가 알아서 낮춘다
    set_level(&core, false);
    MEM.with(|m| {
        if let Some(x) = m.borrow_mut().get_mut(&id) {
            x.suspending = true;
        }
    });
    let handler = TrySuspendCompletedHandler::create(Box::new(move |err, ok| {
        MEM.with(|m| {
            if let Some(x) = m.borrow_mut().get_mut(&id) {
                x.suspending = false;
                if err.is_ok() && ok && x.hidden_since.is_some() {
                    x.phase = Phase::Suspended;
                }
            }
        });
        if !(err.is_ok() && ok) {
            crate::log::warn(format!("탭 {id} 재우기 실패, 다음 주기에 다시 시도"));
        }
        Ok(())
    }));
    if unsafe { c3.TrySuspend(&handler) }.is_err() {
        MEM.with(|m| {
            if let Some(x) = m.borrow_mut().get_mut(&id) {
                x.suspending = false;
            }
        });
    }
}

/// 버리기: 웹뷰를 닫고 주소·제목·파비콘만 남긴다. 다시 볼 때 새 웹뷰로 불러온다.
fn discard(app: &AppHandle, id: TabId) {
    forget(id);
    browser::discard(app, id);
}

/// 탭마다 남겨 둔 것의 (이름, 탭 id). 개발용 `orphans` 확인에 쓴다
#[cfg(debug_assertions)]
pub fn tab_entries() -> Vec<(&'static str, TabId)> {
    MEM.with(|m| m.borrow().keys().map(|id| ("memory.MEM", *id)).collect())
}

/// 지금 상태 (개발용 `mem` 확인에 쓴다)
#[cfg(debug_assertions)]
pub fn phases() -> Vec<(TabId, Phase)> {
    MEM.with(|m| m.borrow().iter().map(|(id, x)| (*id, x.phase)).collect())
}
