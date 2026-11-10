//! 단축키 (spec 5.6). 포커스가 웹 페이지 안에 있을 때는 콘텐츠 웹뷰의 AcceleratorKeyPressed로,
//! UI에 있을 때는 UI의 keydown으로 받아 같은 이름으로 처리한다 (0단계 7번).

use tauri::{AppHandle, Emitter, EventTarget, Manager};

use crate::browser::{self, UI_WEBVIEW};

const VK_TAB: u32 = 0x09;
const VK_LEFT: u32 = 0x25;
const VK_RIGHT: u32 = 0x27;
const VK_F5: u32 = 0x74;
const VK_F11: u32 = 0x7A;

/// 키 조합의 이름. 호스트가 처리하지 않는 키(Ctrl+F, F12 등)는 None으로 페이지에 맡긴다.
pub fn name(vk: u32, ctrl: bool, shift: bool, alt: bool) -> Option<&'static str> {
    let key = char::from_u32(vk).filter(|c| c.is_ascii_uppercase());
    Some(match (ctrl, shift, alt, key, vk) {
        (true, false, false, Some('T'), _) => "new_tab",
        (true, false, false, Some('W'), _) => "close_tab",
        (true, true, false, Some('T'), _) => "reopen_tab",
        (true, false, false, _, VK_TAB) => "next_tab",
        (true, true, false, _, VK_TAB) => "prev_tab",
        (true, false, false, Some('L'), _) => "focus_address",
        (true, false, false, Some('R'), _) | (false, false, false, _, VK_F5) => "reload",
        (false, false, true, _, VK_LEFT) => "back",
        (false, false, true, _, VK_RIGHT) => "forward",
        (true, false, false, Some('D'), _) => "bookmark",
        (true, false, false, Some('H'), _) => "history",
        (true, false, false, Some('J'), _) => "downloads",
        (true, false, false, Some('B'), _) => "toggle_sidebar",
        (true, true, false, Some('A'), _) => "focus_tab_search",
        (false, false, false, _, VK_F11) => "fullscreen",
        _ => return None,
    })
}

/// UI에서 처리하는 단축키: UI 웹뷰에 포커스를 주고 알린다
fn to_ui(app: &AppHandle, name: &str) {
    if let Some(ui) = app.get_webview(UI_WEBVIEW) {
        let _ = ui.set_focus();
    }
    let _ = app.emit_to(EventTarget::webview(UI_WEBVIEW), "shortcut", name);
}

/// 단축키를 실행한다. 메인 스레드에서, WebView2 이벤트 처리기 밖에서 부른다.
pub fn run(app: &AppHandle, name: &str) {
    let active = browser::snapshot(app).active;
    match name {
        "new_tab" => {
            browser::new_tab(app, 0, true);
            // 새 탭 화면의 검색 칸에 포커스 (spec 5.1)
            to_ui(app, "focus_new_tab");
        }
        "close_tab" => {
            if let Some(id) = active {
                browser::close(app, id);
            }
        }
        "reopen_tab" => browser::reopen_closed(app),
        "next_tab" | "prev_tab" => browser::activate_neighbor(app, name == "next_tab"),
        "reload" => {
            if let Some(id) = active {
                browser::reload(app, id);
            }
        }
        "back" => {
            if let Some(id) = active {
                browser::go_back(id);
            }
        }
        "forward" => {
            if let Some(id) = active {
                browser::go_forward(id);
            }
        }
        "bookmark" => browser::bookmark_active(app),
        "history" => browser::open_internal(app, "tbs://history"),
        "downloads" => browser::open_internal(app, "tbs://downloads"),
        "settings" => browser::open_internal(app, "tbs://settings"),
        "fullscreen" => browser::toggle_fullscreen(app),
        "find" => browser::focus_page(app),
        "devtools" => browser::open_devtools(app),
        "focus_address" | "focus_tab_search" | "toggle_sidebar" => to_ui(app, name),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::name;

    #[test]
    fn spec_shortcuts() {
        assert_eq!(name('T' as u32, true, false, false), Some("new_tab"));
        assert_eq!(name('T' as u32, true, true, false), Some("reopen_tab"));
        assert_eq!(name(0x09, true, true, false), Some("prev_tab"));
        assert_eq!(name(0x74, false, false, false), Some("reload"));
        assert_eq!(name(0x25, false, false, true), Some("back"));
        assert_eq!(name('A' as u32, true, true, false), Some("focus_tab_search"));
        assert_eq!(name(0x7A, false, false, false), Some("fullscreen"));
        // 페이지에 맡기는 키
        assert_eq!(name('F' as u32, true, false, false), None);
        assert_eq!(name(0x7B, false, false, false), None);
        assert_eq!(name('C' as u32, true, false, false), None);
    }
}
