//! 데이터 폴더 위치 (spec 8장). 모든 데이터는 %LOCALAPPDATA%\TBs Browser\ 아래에 둔다.

use std::path::PathBuf;

use crate::APP_DIR_NAME;

pub fn data_root() -> PathBuf {
    // 개발용: 개발 중 실행이 사용자의 실제 데이터를 건드리지 않게 다른 폴더를 쓸 수 있다.
    #[cfg(debug_assertions)]
    if let Ok(dir) = std::env::var("TBS_DATA_DIR") {
        return PathBuf::from(dir);
    }
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join(APP_DIR_NAME)
}

/// WebView2 사용자 데이터 폴더 (쿠키, 캐시, 로그인 정보)
pub fn webview_dir() -> PathBuf {
    data_root().join("webview")
}

/// 세션, 북마크, 설정 같은 JSON 파일 폴더
pub fn state_dir() -> PathBuf {
    data_root().join("state")
}
