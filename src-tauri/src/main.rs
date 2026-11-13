// 릴리스 빌드에서는 콘솔 창을 띄우지 않는다
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod adblock;
mod bookmarks;
mod browser;
mod commands;
mod dev;
mod downloads;
mod examples;
mod ext_compat;
mod extensions;
mod frame;
mod history;
mod input;
mod library;
mod log;
mod memory;
mod native;
mod paths;
mod permissions;
mod popup;
mod scripts;
mod session;
mod settings;
mod shortcuts;
mod site_scripts;
mod state;
mod storage;
mod store;
mod updater;

use tauri::{
    webview::WebviewBuilder, window::WindowBuilder, LogicalPosition, LogicalSize, Manager, WebviewUrl,
};

use browser::{Browser, MAIN_WINDOW, UI_WEBVIEW};
use history::History;
use settings::SettingsStore;

/// 화면에 보이는 이름. 이름을 바꿀 때는 이 상수와 APP_DIR_NAME만 고친다.
pub const APP_NAME: &str = "TB's Browser";
/// 파일·폴더 이름처럼 아포스트로피를 쓸 수 없는 곳의 이름.
pub const APP_DIR_NAME: &str = "TBs Browser";

fn main() {
    log::install_panic_hook();
    // WebView2 런타임이 없으면 알리고 끝낸다 (spec 5.7. 설치 파일이 깔지만 지워졌을 수 있다)
    if !log::check_webview2() {
        return;
    }
    let built = tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updater::UpdateState::default())
        .manage(extensions::ExtStore::default())
        .manage(Browser::default())
        .manage(library::Library::default())
        .manage(SettingsStore::default())
        .manage(site_scripts::ScriptStore::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::tab_create,
            commands::tab_close,
            commands::tab_activate,
            commands::navigate,
            commands::go_back,
            commands::go_forward,
            commands::reload,
            commands::stop,
            commands::set_content_bounds,
            commands::set_max_button_bounds,
            commands::tab_open_url,
            commands::tab_move,
            commands::group_create,
            commands::group_rename,
            commands::group_set_collapsed,
            commands::group_ungroup,
            commands::tab_ungroup,
            commands::tab_set_keep_alive,
            commands::bookmarks_get,
            commands::take_notices,
            commands::bookmark_add,
            commands::bookmark_toggle,
            commands::bookmark_add_folder,
            commands::bookmark_rename,
            commands::bookmark_remove,
            commands::bookmark_move,
            commands::bookmark_set_open,
            commands::bookmarks_set_collapsed,
            commands::settings_get,
            commands::settings_set,
            commands::pick_download_dir,
            commands::history_search,
            commands::history_delete,
            commands::history_clear,
            commands::suggest,
            commands::downloads_get,
            commands::download_open,
            commands::download_show,
            commands::download_cancel,
            commands::run_shortcut,
            commands::popup_show,
            commands::popup_hide,
            commands::popup_pick,
            commands::fullscreen_state,
            commands::scripts_list,
            commands::script_set_enabled,
            commands::script_get,
            commands::script_set_value,
            commands::script_open_settings,
            commands::script_run_action,
            commands::script_remove,
            commands::store_list,
            commands::store_install,
            commands::store_confirm,
            commands::script_set_pinned,
            commands::scripts_reload,
            commands::scripts_open_folder,
            commands::sidebar_get,
            commands::sidebar_set_collapsed,
            commands::top_sites,
            commands::permission_requests,
            commands::permission_answer,
            commands::apply_theme,
            commands::popup_menu_pick,
            commands::update_get,
            commands::update_install,
            commands::log_ui,
            commands::ext_list,
            commands::ext_install_pick,
            commands::ext_install_confirm,
            commands::ext_set_enabled,
            commands::ext_set_pinned,
            commands::ext_remove,
            commands::ext_open_popup,
            commands::ext_open_options,
            commands::popup_fit,
            commands::adblock_status,
            commands::adblock_set_site,
            commands::adblock_lists,
            commands::adblock_update_now,
            commands::adblock_reload,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            library::load(&handle);
            if let Some(n) = settings::load(&handle) {
                library::notice(&handle, n);
            }
            let (hist, note) = History::open(&paths::state_dir().join("history.sqlite"));
            hist.mark_unfinished_failed();
            app.manage(hist);
            site_scripts::seed_examples();
            site_scripts::reload(&handle);
            store::start(&handle);
            site_scripts::watch(&handle);
            if let Some(n) = note {
                library::notice(&handle, n);
            }
            // 숨긴 채로 만들고 웹뷰를 다 얹은 뒤에 보인다
            let win = WindowBuilder::new(app, MAIN_WINDOW)
                .title(APP_NAME)
                .inner_size(1280.0, 800.0)
                .min_inner_size(640.0, 400.0)
                .decorations(false)
                .visible(false)
                .focused(!dev::quiet())
                .build()?;
            let size = win.inner_size()?.to_logical::<f64>(win.scale_factor()?);
            let ui = win.add_child(
                WebviewBuilder::new(UI_WEBVIEW, WebviewUrl::App("index.html".into()))
                    .data_directory(paths::webview_dir())
                    .browser_extensions_enabled(true)
                    .focused(!dev::quiet())
                    .auto_resize(),
                LogicalPosition::new(0.0, 0.0),
                LogicalSize::new(size.width, size.height),
            )?;
            // 콘텐츠 웹뷰가 같이 쓸 환경을 UI 웹뷰에서 받아 둔다 (spec 3장 배치)
            ui.with_webview(|p| browser::remember_environment(p.environment()))?;
            frame::install(&handle, &win);
            {
                // 최대화 상태가 바뀌면 UI가 최대화·복원 아이콘을 바꾼다
                let h = handle.clone();
                let w2 = win.clone();
                win.on_window_event(move |e| {
                    // 메인 창을 옮기거나 크기를 바꾸기 시작하면 팝업을 닫는다 (spec 3장 배치).
                    // 포커스를 잃을 때는 닫지 않는다: 팝업을 누르는 순간에도 포커스가 빠져 고르기가 끊긴다.
                    // 다른 창으로 가면 주소창이 포커스를 잃으면서 UI가 닫는다.
                    if matches!(e, tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_)) {
                        let h2 = h.clone();
                        // 확장 팝업은 보통 창이라 따라 닫지 않는다
                        let _ = h.run_on_main_thread(move || popup::hide(&h2));
                    }
                    if let tauri::WindowEvent::Resized(_) = e {
                        browser::emit_ui(&h, "window_maximized", w2.is_maximized().unwrap_or(false));
                    }
                });
            }
            popup::create(&handle, &win)?;
            extensions::start(&handle);
            adblock::start(&handle);
            // 이전 탭 복원 (설정에서 끌 수 있다). 복원하지 않으면 새 탭 하나로 시작한다.
            let restored = if settings::get(&handle).restore_tabs {
                match session::load() {
                    Ok(Some(s)) if !s.tabs.is_empty() => {
                        browser::restore(&handle, s);
                        true
                    }
                    Ok(_) => false,
                    Err(n) => {
                        library::notice(&handle, n);
                        false
                    }
                }
            } else {
                false
            };
            if !restored {
                browser::new_tab(&handle, 0, true);
            }
            session::start(&handle);
            memory::start(&handle);
            updater::start(&handle);
            dev::show_main_window(&win);
            dev::start_hook(&handle);
            Ok(())
        })
        .build(tauri::generate_context!());
    let app = match built {
        Ok(a) => a,
        Err(e) => {
            log::fatal_box(&format!(
                "TB's Browser를 시작하지 못했습니다({e}).

프로그램을 다시 켜 보고, 그래도 안 되면 {} 폴더의 tbs.log를 보내 주세요.",
                log::dir().display()
            ));
            return;
        }
    };
    app.run(|app, event| {
        // 끝낼 때 세션을 한 번 더 저장한다 (spec 8장)
        if let tauri::RunEvent::Exit = event {
            session::save_now(app);
        }
    });
}
