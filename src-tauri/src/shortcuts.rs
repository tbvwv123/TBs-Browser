//! 단축키 (spec 5.6). 포커스가 웹 페이지 안에 있을 때는 콘텐츠 웹뷰의 AcceleratorKeyPressed로,
//! UI에 있을 때는 UI의 keydown으로 받아 같은 이름으로 처리한다 (0단계 7번).

use tauri::{AppHandle, Manager};

use crate::browser::{self, UI_WEBVIEW};

const VK_TAB: u32 = 0x09;
const VK_LEFT: u32 = 0x25;
const VK_RIGHT: u32 = 0x27;
const VK_F5: u32 = 0x74;
const VK_F11: u32 = 0x7A;

/// 브라우저 단축키. 글자 꼴(snake_case)은 UI와 주고받는 이름이다 (src/lib/rules.ts shortcutName, App.svelte)
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shortcut {
    NewTab,
    CloseTab,
    ReopenTab,
    NextTab,
    PrevTab,
    Reload,
    Back,
    Forward,
    Bookmark,
    History,
    Downloads,
    Settings,
    /// 스크립트 찾아보기 (tbs://scripts)
    Store,
    Extensions,
    Fullscreen,
    Find,
    Devtools,
    // UI에서 처리하는 것
    FocusAddress,
    FocusTabSearch,
    ToggleSidebar,
    /// 호스트가 UI에 보내기만 한다 (새 탭을 연 뒤). UI에서 받지 않는다
    #[serde(skip_deserializing)]
    FocusNewTab,
}

/// 키 조합의 단축키. 호스트가 처리하지 않는 키(Ctrl+F, F12 등)는 None으로 페이지에 맡긴다.
/// UI의 shortcutName과 함께 tests/rules-vectors.json(`shortcuts`)으로 시험한다
pub fn name(vk: u32, ctrl: bool, shift: bool, alt: bool) -> Option<Shortcut> {
    use Shortcut::*;
    let key = char::from_u32(vk).filter(|c| c.is_ascii_uppercase());
    Some(match (ctrl, shift, alt, key, vk) {
        (true, false, false, Some('T'), _) => NewTab,
        (true, false, false, Some('W'), _) => CloseTab,
        (true, true, false, Some('T'), _) => ReopenTab,
        (true, false, false, _, VK_TAB) => NextTab,
        (true, true, false, _, VK_TAB) => PrevTab,
        (true, false, false, Some('L'), _) => FocusAddress,
        (true, false, false, Some('R'), _) | (false, false, false, _, VK_F5) => Reload,
        (false, false, true, _, VK_LEFT) => Back,
        (false, false, true, _, VK_RIGHT) => Forward,
        (true, false, false, Some('D'), _) => Bookmark,
        (true, false, false, Some('H'), _) => History,
        (true, false, false, Some('J'), _) => Downloads,
        (true, false, false, Some('B'), _) => ToggleSidebar,
        (true, true, false, Some('A'), _) => FocusTabSearch,
        (false, false, false, _, VK_F11) => Fullscreen,
        _ => return None,
    })
}

/// UI에서 처리하는 단축키: UI 웹뷰에 포커스를 주고 알린다
fn to_ui(app: &AppHandle, sc: Shortcut) {
    if let Some(ui) = app.get_webview(UI_WEBVIEW) {
        let _ = ui.set_focus();
    }
    crate::browser::emit_ui(app, "shortcut", sc);
}

/// 단축키를 실행한다. 메인 스레드에서, WebView2 이벤트 처리기 밖에서 부른다.
pub fn run(app: &AppHandle, sc: Shortcut) {
    use Shortcut::*;
    let active = browser::snapshot(app).active;
    match sc {
        NewTab => {
            browser::new_tab(app, 0, true);
            // 새 탭 화면의 검색 칸에 포커스 (spec 5.1)
            to_ui(app, FocusNewTab);
        }
        CloseTab => {
            if let Some(id) = active {
                browser::close(app, id);
            }
        }
        ReopenTab => browser::reopen_closed(app),
        NextTab | PrevTab => browser::activate_neighbor(app, sc == NextTab),
        Reload => {
            if let Some(id) = active {
                browser::reload(app, id);
            }
        }
        Back => {
            if let Some(id) = active {
                browser::go_back(app, id);
            }
        }
        Forward => {
            if let Some(id) = active {
                browser::go_forward(app, id);
            }
        }
        Bookmark => browser::bookmark_active(app),
        History => browser::open_internal(app, "tbs://history"),
        Downloads => browser::open_internal(app, "tbs://downloads"),
        Settings => browser::open_internal(app, "tbs://settings"),
        Store => browser::open_internal(app, "tbs://scripts"),
        Extensions => browser::open_internal(app, "tbs://extensions"),
        Fullscreen => browser::toggle_fullscreen(app),
        Find => browser::focus_page(app),
        Devtools => browser::open_devtools(app),
        FocusAddress | FocusTabSearch | ToggleSidebar | FocusNewTab => to_ui(app, sc),
    }
}

#[cfg(test)]
mod tests {
    use super::{name, Shortcut};

    #[test]
    fn spec_shortcuts() {
        assert_eq!(name('T' as u32, true, false, false), Some(Shortcut::NewTab));
        assert_eq!(name('T' as u32, true, true, false), Some(Shortcut::ReopenTab));
        assert_eq!(name(0x09, true, true, false), Some(Shortcut::PrevTab));
        assert_eq!(name(0x74, false, false, false), Some(Shortcut::Reload));
        assert_eq!(name(0x25, false, false, true), Some(Shortcut::Back));
        assert_eq!(name('A' as u32, true, true, false), Some(Shortcut::FocusTabSearch));
        assert_eq!(name(0x7A, false, false, false), Some(Shortcut::Fullscreen));
        // 페이지에 맡기는 키
        assert_eq!(name('F' as u32, true, false, false), None);
        assert_eq!(name(0x7B, false, false, false), None);
        assert_eq!(name('C' as u32, true, false, false), None);
    }

    #[test]
    fn shortcut_vectors() {
        let v: serde_json::Value = serde_json::from_str(include_str!("../../tests/rules-vectors.json")).unwrap();
        for c in v["shortcuts"].as_array().unwrap() {
            let b = |k: &str| c[k].as_bool().unwrap();
            let got = name(c["vk"].as_u64().unwrap() as u32, b("ctrl"), b("shift"), b("alt"));
            assert_eq!(serde_json::to_value(got).unwrap(), c["host"], "{c}");
        }
    }

    #[test]
    fn shortcut_names_from_ui() {
        let names = [
            "new_tab", "close_tab", "reopen_tab", "next_tab", "prev_tab", "reload", "back", "forward", "bookmark", "history",
            "downloads", "settings", "store", "extensions", "fullscreen", "find", "devtools", "focus_address",
            "focus_tab_search", "toggle_sidebar",
        ];
        for n in names {
            let sc: Shortcut = serde_json::from_value(n.into()).unwrap();
            assert_eq!(serde_json::to_value(sc).unwrap(), n);
        }
        assert_eq!(serde_json::to_value(Shortcut::FocusNewTab).unwrap(), "focus_new_tab");
        // UI에서 받지 않는 이름
        assert!(serde_json::from_value::<Shortcut>("focus_new_tab".into()).is_err());
        assert!(serde_json::from_value::<Shortcut>("nope".into()).is_err());
    }
}
