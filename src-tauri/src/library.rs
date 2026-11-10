//! 북마크 보관: 메모리의 북마크 트리와 state\bookmarks.json을 맞춘다.
//! 바뀔 때마다 바로 저장하고, UI에 `bookmarks_changed`로 알린다.

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, EventTarget, Manager};

use crate::{bookmarks::Bookmarks, browser::UI_WEBVIEW, paths, storage};

#[derive(Default)]
pub struct Library {
    bookmarks: Mutex<Bookmarks>,
    /// UI가 처음 뜰 때 보여 줄 알림 (파일을 읽지 못했을 때 등)
    notices: Mutex<Vec<String>>,
}

fn file() -> std::path::PathBuf {
    paths::state_dir().join("bookmarks.json")
}

fn lib(app: &AppHandle) -> tauri::State<'_, Library> {
    app.state::<Library>()
}

pub fn load(app: &AppHandle) {
    let loaded = match storage::load::<Bookmarks>(&file()) {
        Ok(Some(b)) => b.migrate(),
        Ok(None) => Bookmarks::default(),
        Err(msg) => {
            lib(app).notices.lock().unwrap().push(msg);
            Bookmarks::default()
        }
    };
    *lib(app).bookmarks.lock().unwrap() = loaded;
}

pub fn get(app: &AppHandle) -> Bookmarks {
    lib(app).bookmarks.lock().unwrap().clone()
}

/// 처음 화면에 보여 줄 알림을 더한다
pub fn notice(app: &AppHandle, msg: String) {
    crate::log::warn(&msg);
    lib(app).notices.lock().unwrap().push(msg);
}

pub fn take_notices(app: &AppHandle) -> Vec<String> {
    std::mem::take(&mut *lib(app).notices.lock().unwrap())
}

/// 북마크를 바꾸고, 저장하고, UI에 알린다.
pub fn edit<T>(app: &AppHandle, f: impl FnOnce(&mut Bookmarks) -> Result<T, String>) -> Result<T, String> {
    let (out, snapshot) = {
        let l = lib(app);
        let mut b = l.bookmarks.lock().unwrap();
        let out = f(&mut b)?;
        (out, b.clone())
    };
    let saved = storage::save(&file(), &snapshot);
    let _ = app.emit_to(EventTarget::webview(UI_WEBVIEW), "bookmarks_changed", &snapshot);
    saved.map(|_| out)
}
