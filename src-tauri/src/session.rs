//! 세션 저장 (spec 5.4, 8장). 파일: state\session.json
//! 바뀐 뒤 1초 동안 더 바뀌지 않으면 저장하고, 끝낼 때 한 번 더 저장한다.

use std::{
    sync::{
        mpsc::{channel, RecvTimeoutError, Sender},
        Mutex, OnceLock,
    },
    time::Duration,
};

use tauri::AppHandle;

use crate::{browser, paths, state::Session, storage};

static TX: OnceLock<Mutex<Sender<()>>> = OnceLock::new();

fn file() -> std::path::PathBuf {
    paths::state_dir().join("session.json")
}

pub fn load() -> Result<Option<Session>, String> {
    storage::load::<Session>(&file())
}

pub fn save_now(app: &AppHandle) {
    let s = browser::session(app);
    if let Err(e) = storage::save(&file(), &s) {
        crate::log::error(format!("{e}"));
    }
}

/// 저장 담당 스레드를 띄운다
pub fn start(app: &AppHandle) {
    let (tx, rx) = channel::<()>();
    let _ = TX.set(Mutex::new(tx));
    let app = app.clone();
    std::thread::spawn(move || {
        while rx.recv().is_ok() {
            // 1초 동안 더 바뀌지 않을 때까지 기다린다
            loop {
                match rx.recv_timeout(Duration::from_secs(1)) {
                    Ok(()) => continue,
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            save_now(&app);
        }
    });
}

/// 세션이 바뀌었다고 알린다
pub fn touch() {
    if let Some(tx) = TX.get() {
        let _ = tx.lock().unwrap().send(());
    }
}
