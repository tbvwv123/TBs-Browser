//! 자동 업데이트 (spec 5.7). 시작할 때와 그 뒤 하루에 한 번 새 버전을 확인한다.
//! 새 버전이 있으면 UI가 점 3개 버튼에 점을 찍고 메뉴에 "업데이트 설치"를 보여준다.
//! 사용자가 누를 때만 내려받아 설치하고 다시 시작한다. 묻지 않고 재시작하지 않는다.

use std::{sync::Mutex, time::Duration};

use tauri::{AppHandle, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::{log, session};

#[derive(Default)]
pub struct UpdateState {
    /// 확인한 새 버전
    available: Mutex<Option<Update>>,
    /// 내려받는 중
    busy: Mutex<bool>,
}

const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

pub fn start(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || loop {
        tauri::async_runtime::block_on(check(&app));
        std::thread::sleep(CHECK_EVERY);
    });
}

/// 새 버전을 확인한다. 인터넷이 없거나 서버가 응답하지 않으면 로그만 남기고 조용히 넘어간다.
async fn check(app: &AppHandle) {
    let builder = app.updater_builder();
    // 개발용: 올라간 버전이 지금 버전과 같아도 새 버전으로 본다 (업데이트 확인·내려받기 시험)
    #[cfg(debug_assertions)]
    let builder = if std::env::var_os("TBS_DEV_FORCE_UPDATE").is_some() { builder.version_comparator(|_, _| true) } else { builder };
    let result = match builder.build() {
        Ok(u) => u.check().await,
        Err(e) => Err(e),
    };
    match result {
        Ok(Some(update)) => {
            log::info(format!("새 버전 있음: {} → {}", update.current_version, update.version));
            let version = update.version.clone();
            *app.state::<UpdateState>().available.lock().unwrap() = Some(update);
            crate::browser::emit_ui(app, "update_available", version);
        }
        Ok(None) => log::info("업데이트 확인: 최신 버전"),
        Err(e) => log::warn(format!("업데이트 확인 실패: {e}")),
    }
}

/// 개발용: 확인해 둔 새 버전을 내려받아 서명만 검사한다. 설치하지 않는다.
#[cfg(debug_assertions)]
pub async fn dev_download(app: &AppHandle) -> String {
    let update = app.state::<UpdateState>().available.lock().unwrap().clone();
    let Some(update) = update else { return "no update".into() };
    match update.download(|_, _| {}, || {}).await {
        Ok(bytes) => format!("downloaded {} bytes, signature ok, version {}", bytes.len(), update.version),
        Err(e) => format!("ERR {e}"),
    }
}

/// 확인해 둔 새 버전 (없으면 None)
pub fn available(app: &AppHandle) -> Option<String> {
    app.state::<UpdateState>().available.lock().unwrap().as_ref().map(|u| u.version.clone())
}

/// 내려받아 설치하고 다시 시작한다. 설치 파일이 프로그램을 닫고 새 버전을 띄운다.
pub async fn install(app: AppHandle) -> Result<(), String> {
    let update = app
        .state::<UpdateState>()
        .available
        .lock()
        .unwrap()
        .clone()
        .ok_or("설치할 업데이트가 없습니다. 프로그램을 다시 시작하면 새 버전을 다시 확인합니다.")?;
    {
        let state = app.state::<UpdateState>();
        let mut busy = state.busy.lock().unwrap();
        if *busy {
            return Err("업데이트를 이미 내려받는 중입니다. 잠시 기다려 주세요.".into());
        }
        *busy = true;
    }
    let app2 = app.clone();
    let mut received = 0u64;
    let bytes = update
        .download(
            move |chunk, total| {
                received += chunk as u64;
                let fraction = total.filter(|t| *t > 0).map(|t| received as f64 / t as f64).unwrap_or(-1.0);
                crate::browser::emit_ui(&app2, "update_progress", fraction);
            },
            || {},
        )
        .await
        .map_err(|e| {
            *app.state::<UpdateState>().busy.lock().unwrap() = false;
            let msg = format!("업데이트를 내려받지 못했습니다({e}). 인터넷 연결을 확인하고 다시 눌러 주세요.");
            log::error(&msg);
            msg
        })?;
    log::info(format!("업데이트 {} 설치 시작", update.version));
    // 설치 파일이 이 프로그램을 끝내므로 지금 탭을 저장해 둔다 (다시 켜지면 세션 복원으로 돌아온다)
    session::save_now(&app);
    update.install(bytes).map_err(|e| {
        *app.state::<UpdateState>().busy.lock().unwrap() = false;
        let msg = format!("업데이트를 설치하지 못했습니다({e}). 잠시 뒤 다시 눌러 주세요.");
        log::error(&msg);
        msg
    })?;
    // Windows에서는 install()이 설치 파일을 띄우고 프로그램을 끝낸다. 여기까지 오면 직접 다시 시작한다.
    app.restart();
}
