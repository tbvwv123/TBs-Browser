fn main() {
    // 앱 명령에 권한을 요구하게 만든다. 이 목록에 있는 명령은 capability에 허용된 웹뷰에서만 불린다.
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&["s0_ping", "set_content_bounds", "show_tab", "ui_log"]),
    ))
    .expect("tauri-build 실패");
}
